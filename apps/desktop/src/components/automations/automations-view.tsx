import { memo, useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import {
  Activity,
  ArrowUpRight,
  CheckCircle2,
  ChevronRight,
  Clock,
  FileText,
  Folder,
  GitBranch,
  GitCompareArrows,
  HeartPulse,
  ListTodo,
  Pause,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  ShieldCheck,
  Square,
  Target,
  Timer,
  Trash2,
  X,
  Zap,
  type LucideIcon,
} from "lucide-react";
import {
  PROVIDER_LIST,
  formatProviderLabel,
  formatTimerConditionLabel,
  formatTimerCountdown,
  type ConditionalTimerRecord,
  type GoalRunRecord,
  type MonitorWatchRecord,
  type ProviderKind,
  type ScheduleRunRecord,
  type ScheduleSpec,
  type SessionMode,
  type TimerCondition,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { isBareHomeDir, isLocalHostCard, normalizeProjectPath } from "@/lib/projects";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog-tabs";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { SimpleTooltip } from "@/components/ui/overlays";
import { useWorkspaceMotion } from "@/components/layout/workspace-motion";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  SettingSelect,
  ToggleSwitch,
  type SettingSelectOption,
} from "@/components/settings/settings-primitives";
import { WorkflowsPanel } from "@/components/automations/workflows-panel";
import { type SlashDirective, orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { EventTriggersSection } from "@/components/automations/event-triggers-section";
import { cn } from "@/lib/utils";

interface AutomationTemplate {
  id: string;
  title: string;
  description: string;
  defaultCron: string;
  icon: LucideIcon;
  slash?: SlashDirective | null;
  prompt: string;
}

const TEMPLATES: AutomationTemplate[] = [
  {
    id: "diff-audit",
    title: "Working Tree Diff Audit",
    description:
      "Inspect uncommitted changes for regressions, edge cases, and architecture drift.",
    defaultCron: "0 * * * *",
    icon: GitCompareArrows,
    slash: "review",
    prompt:
      "Audit the current working tree changes for correctness bugs, edge cases, and adherence to project conventions.",
  },
  {
    id: "repo-brief",
    title: "Daily Repository Brief",
    description:
      "Summarize recent commits, active branches, and open work items across the workspace.",
    defaultCron: "0 9 * * 1-5",
    icon: FileText,
    prompt:
      "Inspect recent git commits, branch status, and uncommitted changes in this repository, then give me a concise engineering status brief and suggested next steps.",
  },
  {
    id: "quality-gate",
    title: "Build, Lint & Test Check",
    description:
      "Run static checks and unit tests on the host and diagnose any failing targets.",
    defaultCron: "0 9 * * *",
    icon: ShieldCheck,
    prompt:
      "Run the project's standard lint, typecheck, and test commands, and report any warnings or failures with concrete fixes.",
  },
  {
    id: "tech-debt-plan",
    title: "Refactor & Cleanup Plan",
    description:
      "Survey hotspots in the module and propose a concise, low-risk refactoring plan.",
    defaultCron: "0 9 * * 1-5",
    icon: ListTodo,
    slash: "plan",
    prompt:
      "Survey the codebase for high-complexity modules or duplicated logic and draft a concise, incremental refactoring plan.",
  },
];

const CUSTOM_CRON_VALUE = "__custom__";

const SCHEDULE_PRESET_OPTIONS: SettingSelectOption<string>[] = [
  { value: "0 9 * * *", label: "Daily · 09:00" },
  { value: "0 9 * * 1-5", label: "Weekdays · 09:00" },
  { value: "0 * * * *", label: "Hourly" },
  { value: "*/30 * * * *", label: "Every 30 min" },
  { value: "0 9 * * 1", label: "Weekly · Mon 09:00" },
  { value: CUSTOM_CRON_VALUE, label: "Custom cron…" },
];

const SCHEDULE_PRESET_VALUES = new Set(
  SCHEDULE_PRESET_OPTIONS.map((o) => o.value).filter((v) => v !== CUSTOM_CRON_VALUE),
);

function normalizePresetShorthand(expr: string): string {
  const clean = expr.trim().toLowerCase();
  if (clean === "hourly" || clean === "@hourly" || clean === "1h") return "0 * * * *";
  if (clean === "30m") return "*/30 * * * *";
  if (clean === "daily" || clean === "@daily") return "0 9 * * *";
  if (clean === "weekdays" || clean === "@weekdays") return "0 9 * * 1-5";
  if (clean === "weekly" || clean === "@weekly") return "0 9 * * 1";
  const dailyMatch = /^daily\s+(\d{1,2}):(\d{2})$/.exec(clean);
  if (dailyMatch) {
    const h = Number(dailyMatch[1]);
    const m = Number(dailyMatch[2]);
    if (h >= 0 && h <= 23 && m >= 0 && m <= 59) return `${m} ${h} * * *`;
  }
  const weekdaysMatch = /^weekdays\s+(\d{1,2}):(\d{2})$/.exec(clean);
  if (weekdaysMatch) {
    const h = Number(weekdaysMatch[1]);
    const m = Number(weekdaysMatch[2]);
    if (h >= 0 && h <= 23 && m >= 0 && m <= 59) return `${m} ${h} * * 1-5`;
  }
  return expr.trim();
}

function matchSchedulePreset(expr: string): string {
  const normalized = normalizePresetShorthand(expr);
  return SCHEDULE_PRESET_VALUES.has(normalized) ? normalized : CUSTOM_CRON_VALUE;
}

function deriveAutomationName(rawPrompt: string): string {
  const firstLine =
    rawPrompt
      .split(/\r?\n/)
      .map((line) => line.replace(/^[\s#>*\-•]+/, "").trim())
      .find((line) => line.length > 0) ?? "";
  if (!firstLine) return "Scheduled Automation";
  const cleaned = firstLine.replace(/[。.:：;；!?！？]+$/, "").trim();
  if (cleaned.length <= 48) return cleaned || "Scheduled Automation";
  const slice = cleaned.slice(0, 48);
  const lastSpace = slice.lastIndexOf(" ");
  const cut = lastSpace >= 20 ? slice.slice(0, lastSpace) : slice;
  return `${cut.trimEnd()}…`;
}

const PROVIDER_OPTIONS: SettingSelectOption<ProviderKind>[] = PROVIDER_LIST.map((p) => ({
  value: p.id,
  label: p.label,
}));

type SimpleSessionMode = "auto" | "accept_edits" | "full_access" | "plan" | "default";

const PERMISSION_OPTIONS: SettingSelectOption<SimpleSessionMode>[] = [
  { value: "auto", label: "Auto", description: "Guardian risk filter" },
  { value: "accept_edits", label: "Accept Edits", description: "Auto-approve file edits" },
  { value: "full_access", label: "Full Access", description: "Unattended edits & commands" },
  { value: "plan", label: "Plan (Read-only)", description: "Inspect without modifying files" },
  { value: "default", label: "Ask", description: "Prompt before tool calls" },
];

type FilterTab = "all" | "active" | "paused";

function shortWorkspaceName(cwd: string): string {
  if (!cwd || cwd === "~") return "~";
  const clean = cwd.replace(/\/+$/, "");
  const idx = clean.lastIndexOf("/");
  return idx >= 0 ? clean.slice(idx + 1) || "/" : clean;
}

function formatModeLabel(mode: SessionMode): string {
  if (typeof mode !== "string") return mode.custom;
  switch (mode) {
    case "auto":
      return "Auto";
    case "accept_edits":
      return "Accept Edits";
    case "full_access":
      return "Full Access";
    case "plan":
      return "Read-only";
    case "default":
      return "Ask";
  }
}

function formatHumanCadence(expr: string): string {
  const clean = expr.trim().toLowerCase();
  if (!clean) return "Custom";
  const mins = /^(\d+)m$/.exec(clean);
  if (mins) return `Every ${mins[1]}m`;
  const hrs = /^(\d+)h$/.exec(clean);
  if (hrs) return hrs[1] === "1" ? "Hourly" : `Every ${hrs[1]}h`;
  const daily = /^daily\s+(\d{1,2}:\d{2})$/.exec(clean);
  if (daily) return `Daily ${daily[1]}`;
  const weekdays = /^weekdays\s+(\d{1,2}:\d{2})$/.exec(clean);
  if (weekdays) return `Weekdays ${weekdays[1]}`;

  const parts = clean.split(/\s+/);
  if (parts.length === 5) {
    const [m, h, dom, mon, dow] = parts;
    const everyMin = /^\*\/(\d+)$/.exec(m);
    if (everyMin && h === "*" && dom === "*" && mon === "*" && dow === "*") {
      return `Every ${everyMin[1]}m`;
    }
    if (m === "0" && h === "*" && dom === "*" && mon === "*" && dow === "*") {
      return "Hourly";
    }
    const everyHr = /^\*\/(\d+)$/.exec(h);
    if (m === "0" && everyHr && dom === "*" && mon === "*" && dow === "*") {
      return `Every ${everyHr[1]}h`;
    }
    if (/^\d{1,2}$/.test(m) && /^\d{1,2}$/.test(h) && dom === "*" && mon === "*") {
      const hh = h.padStart(2, "0");
      const mm = m.padStart(2, "0");
      if (dow === "*") return `Daily ${hh}:${mm}`;
      if (dow === "1-5" || dow === "mon-fri") return `Weekdays ${hh}:${mm}`;
      const dowNames: Record<string, string> = {
        "0": "Sun",
        "1": "Mon",
        "2": "Tue",
        "3": "Wed",
        "4": "Thu",
        "5": "Fri",
        "6": "Sat",
        "7": "Sun",
      };
      if (dowNames[dow]) return `Weekly ${dowNames[dow]} ${hh}:${mm}`;
    }
  }
  return expr.trim();
}

function formatShortTime(iso: string | null): string {
  if (!iso) return "—";
  try {
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) return iso;
    const now = new Date();
    const sameDay =
      d.getFullYear() === now.getFullYear() &&
      d.getMonth() === now.getMonth() &&
      d.getDate() === now.getDate();
    const hm = d.toLocaleTimeString(undefined, {
      hour: "2-digit",
      minute: "2-digit",
      hour12: false,
    });
    if (sameDay) return hm;
    const tomorrow = new Date(now);
    tomorrow.setDate(now.getDate() + 1);
    const isTomorrow =
      d.getFullYear() === tomorrow.getFullYear() &&
      d.getMonth() === tomorrow.getMonth() &&
      d.getDate() === tomorrow.getDate();
    if (isTomorrow) return `Tomorrow ${hm}`;
    return d.toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
      hour12: false,
    });
  } catch {
    return iso;
  }
}

function formatRunDuration(startedAt: string, finishedAt: string | null): string | null {
  if (!finishedAt) return null;
  const start = Date.parse(startedAt);
  const end = Date.parse(finishedAt);
  if (!Number.isFinite(start) || !Number.isFinite(end) || end < start) return null;
  const secs = Math.round((end - start) / 1000);
  if (secs < 1) return "<1s";
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  const rem = secs % 60;
  return rem > 0 ? `${mins}m ${rem}s` : `${mins}m`;
}

function runsSignature(runs: ScheduleRunRecord[]): string {
  return runs.map((r) => `${r.run_id}:${r.status}:${r.finished_at ?? ""}`).join("|");
}

interface ProjectChoice {
  path: string;
  name: string;
  branch: string | null;
}

export function AutomationsView() {
  const fleet = useOrbitSelector((s) => s.fleet);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const hostDetail = useOrbitSelector((s) =>
    s.selectedHostId ? (s.hostDetails[s.selectedHostId] ?? null) : null,
  );
  const goalRuns: GoalRunRecord[] = useMemo(
    () => hostDetail?.goal_runs ?? [],
    [hostDetail?.goal_runs],
  );
  const monitorWatches: MonitorWatchRecord[] = useMemo(
    () => hostDetail?.monitor_watches ?? [],
    [hostDetail?.monitor_watches],
  );
  const workflowRuns = useMemo(
    () => hostDetail?.workflow_runs ?? [],
    [hostDetail?.workflow_runs],
  );
  const activeWorkflowCount = useMemo(
    () => workflowRuns.filter((r) => r.status === "pending" || r.status === "running").length,
    [workflowRuns],
  );
  const activeGoalCount = useMemo(
    () =>
      goalRuns.filter(
        (g) => g.status === "running" || g.status === "verifying" || g.status === "paused",
      ).length,
    [goalRuns],
  );
  const activeMonitorCount = useMemo(
    () =>
      monitorWatches.filter((m) => m.status === "active" || m.status === "triggered").length,
    [monitorWatches],
  );
  const eventTriggerCount = useOrbitSelector((s) =>
    s.selectedHostId ? (s.eventTriggersByHost[s.selectedHostId]?.triggers.length ?? 0) : 0,
  );
  const [sectionTab, setSectionTab] = useState<
    "schedules" | "triggers" | "live_runs" | "workflows"
  >("schedules");
  useEffect(() => {
    if (selectedHostId) {
      void orbitActions.refreshGoalsAndMonitors(selectedHostId);
    }
  }, [selectedHostId]);
  const gitReposCache = useOrbitSelector((s) => s.gitReposCache);
  const sidebarView = useOrbitSelector((s) => s.sidebarView);
  const draftProvider = useOrbitSelector((s) => s.draftProvider);

  const [filterTab, setFilterTab] = useState<FilterTab>("all");
  const [dialogOpen, setDialogOpen] = useState(false);
  const [editingSchedule, setEditingSchedule] = useState<ScheduleSpec | null>(null);
  const [submitting, setSubmitting] = useState(false);

  // Dialog form state
  const [name, setName] = useState("");
  const [schedulePreset, setSchedulePreset] = useState("0 9 * * *");
  const [customCronExpr, setCustomCronExpr] = useState("0 9 * * *");
  const [prompt, setPrompt] = useState("");
  const [targetProjectPath, setTargetProjectPath] = useState("");
  const [provider, setProvider] = useState<ProviderKind>(draftProvider);
  const [profileId, setProfileId] = useState<string>("__none__");
  const [mode, setMode] = useState<SimpleSessionMode>("accept_edits");
  const [worktree, setWorktree] = useState(true);
  const [worktreeRepoPath, setWorktreeRepoPath] = useState<string>("");
  const [archiveOnFinish, setArchiveOnFinish] = useState(true);
  const [advancedOpen, setAdvancedOpen] = useState(false);

  // List interaction state
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [runsBySchedule, setRunsBySchedule] = useState<Record<string, ScheduleRunRecord[]>>({});
  const [loadingRunsId, setLoadingRunsId] = useState<string | null>(null);
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const runsPollTokenRef = useRef(0);

  // Conditional Timers creation dialog state
  const [timerDialogOpen, setTimerDialogOpen] = useState(false);
  const [timerAgentId, setTimerAgentId] = useState("");
  const [timerDurationSec, setTimerDurationSec] = useState("300");
  const [timerConditionKind, setTimerConditionKind] = useState<TimerCondition["kind"]>("any_message");
  const [timerConditionArg, setTimerConditionArg] = useState("");
  const [timerPrompt, setTimerPrompt] = useState("");
  const [timerSubmitting, setTimerSubmitting] = useState(false);
  const [timerBusyId, setTimerBusyId] = useState<string | null>(null);
  const [timerNowMs, setTimerNowMs] = useState(() => Date.now());

  const conditionalTimers = useMemo(
    () => hostDetail?.conditional_timers ?? [],
    [hostDetail?.conditional_timers],
  );
  const armedTimerCount = useMemo(
    () => conditionalTimers.filter((t) => t.state === "armed").length,
    [conditionalTimers],
  );
  const hasArmedTimers = armedTimerCount > 0;
  const activeAgentChoices = useMemo(
    () =>
      (hostDetail?.agents ?? []).filter(
        (a) => !a.archived_at_rfc3339 && !a.background && !a.parent_agent_id,
      ),
    [hostDetail?.agents],
  );

  useEffect(() => {
    if (!hasArmedTimers) return;
    setTimerNowMs(Date.now());
    const id = window.setInterval(() => setTimerNowMs(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [hasArmedTimers]);

  useEffect(() => {
    setTimerDialogOpen(false);
    setTimerAgentId("");
    setTimerPrompt("");
    setTimerConditionArg("");
    if (!selectedHostId) return;
    void orbitActions.refreshConditionalTimers(selectedHostId, null, true).catch(() => {});
  }, [selectedHostId]);

  const openTimerDialog = useCallback(() => {
    const defaultAgent =
      (selectedAgentId && activeAgentChoices.some((a) => a.agent_id === selectedAgentId)
        ? selectedAgentId
        : activeAgentChoices[0]?.agent_id) ?? "";
    setTimerAgentId(defaultAgent);
    setTimerDurationSec("300");
    setTimerConditionKind("any_message");
    setTimerConditionArg("");
    setTimerPrompt("");
    setTimerDialogOpen(true);
  }, [selectedAgentId, activeAgentChoices]);

  const submitConditionalTimer = useCallback(async () => {
    if (!selectedHostId || timerSubmitting) return;
    const secs = Math.floor(Number(timerDurationSec));
    if (!Number.isFinite(secs) || secs < 1 || secs > 86400) {
      orbitActions.showToast("Duration must be between 1 and 86400 seconds");
      return;
    }
    if (!timerAgentId) {
      orbitActions.showToast("Select a target agent first");
      return;
    }
    const cleanPrompt = timerPrompt.trim();
    if (!cleanPrompt) {
      orbitActions.showToast("Enter a wakeup prompt");
      return;
    }
    let condition: TimerCondition;
    switch (timerConditionKind) {
      case "never":
        condition = { kind: "never" };
        break;
      case "any_message":
        condition = { kind: "any_message" };
        break;
      case "all_subagents_settled":
        condition = { kind: "all_subagents_settled" };
        break;
      case "all_peers_settled":
        condition = { kind: "all_peers_settled" };
        break;
      case "sender": {
        const sid = timerConditionArg.trim();
        if (!sid) {
          orbitActions.showToast("Enter a sender agent ID");
          return;
        }
        condition = { kind: "sender", sender_id: sid };
        break;
      }
      case "file_changed": {
        const path = timerConditionArg.trim();
        if (!path) {
          orbitActions.showToast("Enter a workspace-relative file path");
          return;
        }
        condition = { kind: "file_changed", path };
        break;
      }
    }
    setTimerSubmitting(true);
    try {
      await orbitActions.createConditionalTimer({
        hostId: selectedHostId,
        agentId: timerAgentId,
        durationSeconds: secs,
        prompt: cleanPrompt,
        condition,
      });
      orbitActions.showToast("One-shot timer armed");
      setTimerDialogOpen(false);
    } catch (err) {
      orbitActions.showToast(`Failed to arm timer: ${String(err)}`);
    } finally {
      setTimerSubmitting(false);
    }
  }, [
    selectedHostId,
    timerSubmitting,
    timerDurationSec,
    timerAgentId,
    timerPrompt,
    timerConditionKind,
    timerConditionArg,
  ]);
  const deleteConfirmTimerRef = useRef<number | null>(null);

  const allSchedules = useMemo(
    () => hostDetail?.schedules ?? [],
    [hostDetail?.schedules],
  );
  const cronSchedules = useMemo(
    () => allSchedules.filter((s) => s.kind === "schedule"),
    [allSchedules],
  );
  const heartbeats = useMemo(
    () => allSchedules.filter((s) => s.kind === "heartbeat"),
    [allSchedules],
  );
  const profiles = hostDetail?.profiles ?? [];
  const providerSelectOptions = useMemo<SettingSelectOption<ProviderKind>[]>(() => {
    const installed = hostDetail?.host.available_providers ?? [];
    if (installed.length === 0) return PROVIDER_OPTIONS;
    const installedOpts = PROVIDER_LIST.filter((p) => installed.includes(p.id)).map((p) => ({
      value: p.id,
      label: p.label,
    }));
    const currentMissing = !installedOpts.some((o) => o.value === provider);
    if (currentMissing) {
      installedOpts.push({ value: provider, label: formatProviderLabel(provider) });
    }
    return installedOpts;
  }, [hostDetail?.host.available_providers, provider]);

  const activeCount = useMemo(
    () => cronSchedules.filter((s) => s.status === "active").length,
    [cronSchedules],
  );
  const pausedCount = useMemo(
    () => cronSchedules.filter((s) => s.status === "paused").length,
    [cronSchedules],
  );

  const visibleSchedules = useMemo(() => {
    if (filterTab === "active") return cronSchedules.filter((s) => s.status === "active");
    if (filterTab === "paused") return cronSchedules.filter((s) => s.status === "paused");
    return cronSchedules;
  }, [cronSchedules, filterTab]);

  // Available projects on the active host for the dialog selector
  const hostProjects = useMemo<ProjectChoice[]>(() => {
    const out: ProjectChoice[] = [];
    const seen = new Set<string>();
    const push = (rawPath: string, label: string, branch: string | null) => {
      const clean = normalizeProjectPath(rawPath);
      if (!clean || isBareHomeDir(clean) || clean.includes("/worktrees/")) return;
      if (seen.has(clean)) return;
      seen.add(clean);
      out.push({ path: clean, name: label || shortWorkspaceName(clean), branch });
    };
    for (const p of sidebarView?.projects ?? []) {
      if (selectedHostId && p.host_id !== selectedHostId) continue;
      if (p.is_home_workspace) continue;
      push(p.primary_cwd, p.name, p.git_branch ?? null);
    }
    for (const lp of sidebarView?.launchpad_projects ?? []) {
      if (selectedHostId && lp.host_id !== selectedHostId) continue;
      push(lp.path, lp.name, lp.branch ?? null);
    }
    if (selectedCwd && selectedHostId) {
      const cleanSel = normalizeProjectPath(selectedCwd);
      const belongsToHost =
        out.some((p) => p.path === cleanSel) ||
        (hostDetail?.workspaces ?? []).some((w) => normalizeProjectPath(w.cwd) === cleanSel) ||
        (hostDetail?.discovered_projects ?? []).some(
          (d) => normalizeProjectPath(d.path) === cleanSel,
        );
      if (belongsToHost) {
        push(cleanSel, shortWorkspaceName(cleanSel), null);
      }
    }
    return out;
  }, [sidebarView, selectedHostId, selectedCwd, hostDetail]);

  const hostScopedSelectedCwd = useMemo(() => {
    if (!selectedCwd || !selectedHostId) return "";
    const cleanSel = normalizeProjectPath(selectedCwd);
    return hostProjects.some((p) => p.path === cleanSel) ? cleanSel : "";
  }, [selectedCwd, selectedHostId, hostProjects]);

  const effectiveProjectPath = useMemo(() => {
    const chosen = normalizeProjectPath(targetProjectPath);
    if (chosen && chosen !== "~") return chosen;
    const cleanSel = normalizeProjectPath(selectedCwd || "");
    if (cleanSel && cleanSel !== "~") return cleanSel;
    return hostProjects[0]?.path ?? "~";
  }, [targetProjectPath, selectedCwd, hostProjects]);

  const cachedInspection =
    (selectedHostId ? gitReposCache[`${selectedHostId}::${effectiveProjectPath}`] : undefined) ??
    hostDetail?.git_repos_by_cwd?.[effectiveProjectPath] ??
    null;

  useEffect(() => {
    if (!dialogOpen || editingSchedule || !selectedHostId || effectiveProjectPath === "~") return;
    if (!cachedInspection) {
      void orbitActions.inspectGitRepos(selectedHostId, effectiveProjectPath);
    }
  }, [dialogOpen, editingSchedule, selectedHostId, effectiveProjectPath, cachedInspection]);

  const { isDirectGitRepo, nestedGitRepos } = useMemo(() => {
    const sp = sidebarView?.projects.find(
      (p) =>
        p.host_id === selectedHostId &&
        normalizeProjectPath(p.primary_cwd) === effectiveProjectPath,
    );
    const dp = hostDetail?.discovered_projects.find(
      (p) => normalizeProjectPath(p.path) === effectiveProjectPath,
    );
    const direct =
      cachedInspection?.is_git_repo ?? sp?.is_git_repo ?? dp?.is_git_repo ?? false;
    const nested =
      cachedInspection?.nested_git_repos ??
      sp?.nested_git_repos ??
      dp?.nested_git_repos ??
      [];
    return { isDirectGitRepo: direct, nestedGitRepos: nested };
  }, [cachedInspection, sidebarView, selectedHostId, effectiveProjectPath, hostDetail]);

  const canUseWorktree = isDirectGitRepo || nestedGitRepos.length > 0;
  const effectiveWorktree = canUseWorktree && worktree;

  useEffect(() => {
    return () => {
      if (deleteConfirmTimerRef.current !== null) {
        window.clearTimeout(deleteConfirmTimerRef.current);
      }
    };
  }, []);

  const loadRunsForSchedule = useCallback(
    async (scheduleId: string, prevRuns?: ScheduleRunRecord[]) => {
      if (!selectedHostId) return;
      const token = ++runsPollTokenRef.current;
      const prevSig = prevRuns ? runsSignature(prevRuns) : "";
      setLoadingRunsId(scheduleId);

      try {
        await orbitIpc.fetchScheduleRuns(selectedHostId, scheduleId, 20);
        const delays = [90, 220, 480, 900];
        let latest: ScheduleRunRecord[] = prevRuns ?? [];
        for (const delay of delays) {
          await new Promise((r) => window.setTimeout(r, delay));
          if (runsPollTokenRef.current !== token) return;
          const fetched = await orbitIpc.getScheduleRuns(selectedHostId, scheduleId);
          latest = fetched;
          const nextSig = runsSignature(fetched);
          setRunsBySchedule((prev) => ({ ...prev, [scheduleId]: fetched }));
          if (fetched.length > 0 && nextSig !== prevSig) {
            break;
          }
          if (fetched.length > 0 && prevSig === "") {
            break;
          }
        }
        if (runsPollTokenRef.current === token) {
          setRunsBySchedule((prev) => ({ ...prev, [scheduleId]: latest }));
        }
      } finally {
        if (runsPollTokenRef.current === token) {
          setLoadingRunsId((cur) => (cur === scheduleId ? null : cur));
        }
      }
    },
    [selectedHostId],
  );

  // Automatically refresh expanded runs when the schedule's run_count or last_run_at updates
  const expandedScheduleMeta = useMemo(() => {
    if (!expandedId) return null;
    const sched = cronSchedules.find((s) => s.schedule_id === expandedId);
    return sched ? `${sched.run_count}:${sched.last_run_at ?? ""}` : null;
  }, [cronSchedules, expandedId]);

  useEffect(() => {
    if (!expandedId || !expandedScheduleMeta) return;
    void loadRunsForSchedule(expandedId, runsBySchedule[expandedId]);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [expandedId, expandedScheduleMeta]);

  const openCreateDialog = useCallback(
    (tpl?: AutomationTemplate) => {
      const initialCron = tpl ? tpl.defaultCron : "0 9 * * *";
      const matched = matchSchedulePreset(initialCron);
      setEditingSchedule(null);
      setName(tpl ? tpl.title : "");
      setSchedulePreset(matched);
      setCustomCronExpr(normalizePresetShorthand(initialCron));
      setPrompt(tpl ? tpl.prompt : "");
      setTargetProjectPath(
        normalizeProjectPath(selectedCwd || "") !== "~"
          ? normalizeProjectPath(selectedCwd)
          : (hostProjects[0]?.path ?? "~"),
      );
      setProvider(draftProvider);
      setProfileId("__none__");
      setMode("accept_edits");
      setWorktree(true);
      setWorktreeRepoPath("");
      setArchiveOnFinish(true);
      setAdvancedOpen(false);
      setDialogOpen(true);
    },
    [selectedCwd, hostProjects, draftProvider],
  );

  const openEditDialog = useCallback((sched: ScheduleSpec) => {
    const matched = matchSchedulePreset(sched.cron_expr);
    setEditingSchedule(sched);
    setName(sched.name);
    setSchedulePreset(matched);
    setCustomCronExpr(sched.cron_expr);
    setPrompt(sched.prompt);
    if (sched.target.type === "new_agent") {
      setTargetProjectPath(normalizeProjectPath(sched.target.cwd));
      setProvider(sched.target.provider);
      setProfileId(sched.target.profile_id || "__none__");
      setMode(
        typeof sched.target.mode === "string"
          ? (sched.target.mode as SimpleSessionMode)
          : "accept_edits",
      );
      setWorktree(sched.target.worktree);
      setArchiveOnFinish(sched.target.archive_on_finish);
    }
    setAdvancedOpen(false);
    setDialogOpen(true);
  }, []);

  const effectiveCronExpr =
    schedulePreset === CUSTOM_CRON_VALUE ? customCronExpr.trim() : schedulePreset;

  const handleSubmitDialog = async () => {
    if (!selectedHostId || submitting) return;
    if (!effectiveCronExpr || !prompt.trim()) {
      orbitActions.showToast("Fill in schedule and prompt");
      return;
    }
    const normalizedCronExpr = normalizePresetShorthand(effectiveCronExpr);
    const resolvedName = name.trim() || deriveAutomationName(prompt);
    setSubmitting(true);
    try {
      if (editingSchedule) {
        await orbitIpc.updateSchedule({
          hostId: selectedHostId,
          scheduleId: editingSchedule.schedule_id,
          name: resolvedName,
          cronExpr: normalizedCronExpr,
          prompt: prompt.trim(),
          archiveOnFinish,
        });
        orbitActions.showToast("Automation updated");
      } else {
        const targetCwd =
          effectiveWorktree && !isDirectGitRepo && nestedGitRepos.length > 0
            ? worktreeRepoPath || nestedGitRepos[0].path
            : effectiveProjectPath || "~";
        await orbitIpc.createSchedule({
          hostId: selectedHostId,
          name: resolvedName,
          cronExpr: normalizedCronExpr,
          prompt: prompt.trim(),
          target: {
            type: "new_agent",
            cwd: targetCwd,
            provider,
            profile_id: profileId === "__none__" ? null : profileId,
            mode,
            worktree: effectiveWorktree,
            archive_on_finish: archiveOnFinish,
          },
        });
        orbitActions.showToast("Automation created");
      }
      setDialogOpen(false);
    } catch (err) {
      orbitActions.showToast(`Failed to save automation: ${String(err)}`);
    } finally {
      setSubmitting(false);
    }
  };

  const requestDeleteConfirm = useCallback((scheduleId: string) => {
    if (deleteConfirmTimerRef.current !== null) {
      window.clearTimeout(deleteConfirmTimerRef.current);
    }
    setConfirmDeleteId(scheduleId);
    deleteConfirmTimerRef.current = window.setTimeout(() => {
      setConfirmDeleteId((cur) => (cur === scheduleId ? null : cur));
      deleteConfirmTimerRef.current = null;
    }, 4000);
  }, []);

  const confirmDelete = useCallback(
    async (sched: ScheduleSpec) => {
      if (!selectedHostId) return;
      if (deleteConfirmTimerRef.current !== null) {
        window.clearTimeout(deleteConfirmTimerRef.current);
        deleteConfirmTimerRef.current = null;
      }
      setConfirmDeleteId(null);
      if (expandedId === sched.schedule_id) {
        setExpandedId(null);
      }
      await orbitIpc.deleteSchedule(selectedHostId, sched.schedule_id);
      orbitActions.showToast(`Deleted "${sched.name}"`);
    },
    [selectedHostId, expandedId],
  );

  const projectSelectOptions = useMemo<SettingSelectOption<string>[]>(
    () =>
      hostProjects.map((p) => ({
        value: p.path,
        label: p.name,
        description: p.branch ?? undefined,
      })),
    [hostProjects],
  );

  const profileSelectOptions = useMemo<SettingSelectOption<string>[]>(
    () => [
      { value: "__none__", label: "Default" },
      ...profiles.map((p) => ({
        value: p.profile_id,
        label: p.name,
        description: formatProviderLabel(p.provider),
      })),
    ],
    [profiles],
  );

  const nestedRepoSelectOptions = useMemo<SettingSelectOption<string>[]>(
    () =>
      nestedGitRepos.map((repo) => ({
        value: repo.path,
        label: repo.relative_path || repo.name,
        description: repo.git_branch ?? undefined,
      })),
    [nestedGitRepos],
  );

  const multiHost = fleet.hosts.length > 1;
  const activeHostCard = fleet.hosts.find((h) => h.host_id === selectedHostId) ?? null;
  const activeHostLabel =
    hostDetail?.host.display_name ?? activeHostCard?.display_name ?? "Host";
  const activeHostOnline = Boolean(activeHostCard?.connection_state.startsWith("online"));
  const showHostContext =
    multiHost || (activeHostCard !== null && !isLocalHostCard(activeHostCard));
  const humanCronPreview = formatHumanCadence(customCronExpr);

  const advancedSummary = useMemo(() => {
    if (editingSchedule) {
      return archiveOnFinish ? "Auto-archive on" : "Auto-archive off";
    }
    const parts: string[] = [formatProviderLabel(provider)];
    const selectedProfile =
      profileId !== "__none__" ? profiles.find((p) => p.profile_id === profileId) : null;
    if (selectedProfile) {
      parts.push(selectedProfile.name);
    }
    parts.push(formatModeLabel(mode));
    parts.push(effectiveWorktree ? "Worktree" : "Local");
    if (!archiveOnFinish) {
      parts.push("Keep in sidebar");
    }
    return parts.join(" · ");
  }, [editingSchedule, archiveOnFinish, provider, profileId, profiles, mode, effectiveWorktree]);

  const { stageShiftPx, stageTransition, stageMaxContentWidthPx, isAnimating } =
    useWorkspaceMotion();
  const stagePushStyle = useMemo<CSSProperties>(
    () => ({
      transform: stageShiftPx !== 0 ? `translate3d(${stageShiftPx}px, 0, 0)` : undefined,
      transition: stageTransition,
      willChange: isAnimating ? "transform" : undefined,
    }),
    [stageShiftPx, stageTransition, isAnimating],
  );

  return (
    <ScrollArea
      className="orbit-stage-enter min-h-0 flex-1"
      viewportClassName="px-6 py-6"
      style={
        stageMaxContentWidthPx
          ? ({ "--orbit-stage-max-w": `${stageMaxContentWidthPx}px` } as CSSProperties)
          : undefined
      }
    >
      <div
        style={stagePushStyle}
        className="mx-auto w-full max-w-[var(--orbit-stage-max-w,768px)] space-y-6"
      >
        {/* Top-level 4-Tab Navigation: Schedules · Triggers · Live Runs · Workflows */}
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="bg-muted/70 inline-flex items-center gap-0.5 rounded-[9px] p-0.5">
            {(
              [
                { id: "schedules", label: "Schedules", count: cronSchedules.length },
                { id: "triggers", label: "Triggers", count: eventTriggerCount },
                {
                  id: "live_runs",
                  label: "Live Runs",
                  count: activeGoalCount + activeMonitorCount + armedTimerCount,
                  pulsing: activeGoalCount + activeMonitorCount + armedTimerCount > 0,
                },
                {
                  id: "workflows",
                  label: "Workflows",
                  count: activeWorkflowCount > 0 ? activeWorkflowCount : workflowRuns.length,
                  pulsing: activeWorkflowCount > 0,
                },
              ] as const
            ).map((tab) => {
              const selected = sectionTab === tab.id;
              return (
                <button
                  key={tab.id}
                  type="button"
                  onClick={() => setSectionTab(tab.id)}
                  className={cn(
                    "inline-flex h-[26px] cursor-pointer items-center gap-1.5 rounded-[7px] px-2.5 text-[12px] font-medium transition-colors",
                    selected
                      ? "bg-background text-foreground shadow-2xs"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  {"pulsing" in tab && tab.pulsing && (
                    <span className="size-1.5 rounded-full bg-emerald-500 animate-pulse" />
                  )}
                  <span>{tab.label}</span>
                  {tab.count > 0 && (
                    <span className="text-muted-foreground/75 tabular-nums text-[11px] font-normal">
                      {tab.count}
                    </span>
                  )}
                </button>
              );
            })}
          </div>

          <div className="flex items-center gap-2.5">
            {showHostContext && (
              <span className="text-muted-foreground inline-flex items-center gap-1.5 text-[12px]">
                <span
                  className={cn(
                    "size-1.5 shrink-0 rounded-full",
                    activeHostOnline ? "bg-emerald-500" : "bg-zinc-400",
                  )}
                />
                <span className="max-w-36 truncate">{activeHostLabel}</span>
              </span>
            )}
            {sectionTab === "schedules" && cronSchedules.length > 0 && (
              <button
                type="button"
                disabled={!selectedHostId}
                onClick={() => openCreateDialog()}
                className="bg-foreground text-background hover:opacity-90 inline-flex h-[28px] cursor-pointer items-center gap-1.5 rounded-[8px] px-2.5 text-[12px] font-medium transition-opacity disabled:cursor-not-allowed disabled:opacity-40"
              >
                <Plus className="size-3.5 stroke-[2]" />
                <span>New</span>
              </button>
            )}
          </div>
        </div>

        {sectionTab === "workflows" ? (
          <WorkflowsPanel
            hostId={selectedHostId}
            defaultProvider={draftProvider}
            defaultCwd={hostScopedSelectedCwd || hostProjects[0]?.path || ""}
            projectChoices={hostProjects}
            runs={workflowRuns}
          />
        ) : sectionTab === "triggers" ? (
          <EventTriggersSection
            hostId={selectedHostId}
            availableProviders={hostDetail?.host.available_providers ?? []}
            activeAgents={activeAgentChoices}
            hostProjects={hostProjects}
            defaultCwd={effectiveProjectPath}
            defaultProvider={draftProvider}
          />
        ) : sectionTab === "schedules" ? (
          <>
        {/* Quiet inline filter strip (avoids stacked double segmented controls) */}
        {cronSchedules.length > 0 && (
          <div className="flex items-center gap-1">
            {(
              [
                { id: "all", label: "All", count: cronSchedules.length },
                { id: "active", label: "Active", count: activeCount },
                { id: "paused", label: "Paused", count: pausedCount },
              ] as const
            ).map((tab) => {
              const selected = filterTab === tab.id;
              return (
                <button
                  key={tab.id}
                  type="button"
                  onClick={() => setFilterTab(tab.id)}
                  className={cn(
                    "inline-flex h-[24px] cursor-pointer items-center gap-1 rounded-[6px] px-2 text-[12px] transition-colors",
                    selected
                      ? "bg-black/[0.06] text-foreground font-medium dark:bg-white/[0.08]"
                      : "text-muted-foreground hover:text-foreground hover:bg-black/[0.03] dark:hover:bg-white/[0.04]",
                  )}
                >
                  <span>{tab.label}</span>
                  <span className="text-muted-foreground/70 tabular-nums text-[11px]">
                    {tab.count}
                  </span>
                </button>
              );
            })}
          </div>
        )}

        {/* Recurring Schedules — Hairline Grouped List or Borderless Empty Hero */}
        {cronSchedules.length === 0 ? (
          <div className="space-y-8 pt-6">
            <div className="flex flex-col items-center justify-center px-4 py-4 text-center">
              <div className="bg-muted/70 text-muted-foreground mb-3.5 inline-flex size-9 items-center justify-center rounded-xl">
                <Clock className="size-[17px] stroke-[1.75]" />
              </div>
              <h2 className="text-foreground text-[16px] font-medium tracking-[-0.015em]">
                No automations yet
              </h2>
              <p className="text-muted-foreground mt-1.5 max-w-md text-[13px] leading-relaxed">
                Run background audits, test suites, or repository briefs on a schedule
                {showHostContext ? (
                  <>
                    {" "}
                    on <span className="text-foreground/85">{activeHostLabel}</span>
                  </>
                ) : null}
                .
              </p>
              <button
                type="button"
                disabled={!selectedHostId}
                onClick={() => openCreateDialog()}
                className="bg-foreground text-background hover:opacity-90 mt-5 inline-flex h-[32px] cursor-pointer items-center gap-1.5 rounded-[9px] px-3.5 text-[12.5px] font-medium transition-opacity disabled:cursor-not-allowed disabled:opacity-40"
              >
                <Plus className="size-3.5 stroke-[2]" />
                <span>New automation</span>
              </button>
            </div>

            {/* Starter Templates — Clean 2×2 grid, hover-revealed one-off run */}
            <div className="space-y-2.5">
              <div className="text-muted-foreground/75 px-0.5 text-[11.5px] font-medium">
                Start from a template
              </div>
              <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
                {TEMPLATES.map((tpl) => {
                  const Icon = tpl.icon;
                  return (
                    <div
                      key={tpl.id}
                      role="button"
                      tabIndex={0}
                      onClick={() => openCreateDialog(tpl)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter" || e.key === " ") {
                          e.preventDefault();
                          openCreateDialog(tpl);
                        }
                      }}
                      className="border-border/75 bg-elevated/35 hover:bg-elevated/80 group flex cursor-pointer flex-col justify-between gap-2.5 rounded-xl border px-3.5 py-3 text-left transition-colors outline-none focus-visible:ring-1 focus-visible:ring-foreground/20"
                    >
                      <div className="flex items-center justify-between gap-2">
                        <div className="flex min-w-0 items-center gap-2">
                          <Icon className="text-muted-foreground size-3.5 shrink-0 stroke-[1.75]" />
                          <span className="text-foreground truncate text-[13px] font-medium">
                            {tpl.title}
                          </span>
                        </div>
                        <div className="relative flex shrink-0 items-center">
                          <span className="text-muted-foreground/70 font-mono text-[11px] transition-opacity duration-150 group-hover:opacity-0">
                            {formatHumanCadence(tpl.defaultCron)}
                          </span>
                          <button
                            type="button"
                            onClick={(e) => {
                              e.stopPropagation();
                              orbitActions.startNewThreadDraft(
                                selectedHostId ?? undefined,
                                selectedCwd || undefined,
                                tpl.prompt,
                                tpl.slash ?? null,
                              );
                            }}
                            className="text-muted-foreground hover:text-foreground absolute right-0 inline-flex cursor-pointer items-center gap-0.5 rounded px-1.5 py-0.5 text-[11.5px] whitespace-nowrap opacity-0 transition-opacity duration-150 group-hover:opacity-100 hover:bg-black/[0.05] dark:hover:bg-white/[0.06]"
                          >
                            <span>Run once</span>
                            <ArrowUpRight className="size-3" />
                          </button>
                        </div>
                      </div>
                      <p className="text-muted-foreground/85 line-clamp-2 text-[12px] leading-relaxed">
                        {tpl.description}
                      </p>
                    </div>
                  );
                })}
              </div>
            </div>
          </div>
        ) : visibleSchedules.length === 0 ? (
          <div className="border-border/75 text-muted-foreground rounded-xl border border-dashed px-4 py-8 text-center text-[12.5px]">
            No {filterTab} automations.
          </div>
        ) : (
          <div className="border-border/80 bg-elevated/35 divide-border/70 divide-y overflow-hidden rounded-xl border">
            {visibleSchedules.map((sched) => (
              <ScheduleListRow
                key={sched.schedule_id}
                sched={sched}
                selectedHostId={selectedHostId}
                isExpanded={expandedId === sched.schedule_id}
                isLoadingRuns={loadingRunsId === sched.schedule_id}
                runs={runsBySchedule[sched.schedule_id]}
                isConfirmingDelete={confirmDeleteId === sched.schedule_id}
                onToggleExpand={() =>
                  setExpandedId((cur) => (cur === sched.schedule_id ? null : sched.schedule_id))
                }
                onRefreshRuns={() =>
                  void loadRunsForSchedule(sched.schedule_id, runsBySchedule[sched.schedule_id])
                }
                onRunNow={() => {
                  if (!selectedHostId) return;
                  void orbitIpc.runScheduleOnce(selectedHostId, sched.schedule_id);
                  orbitActions.showToast(`Triggered "${sched.name}"`);
                  if (expandedId === sched.schedule_id) {
                    void loadRunsForSchedule(sched.schedule_id, runsBySchedule[sched.schedule_id]);
                  }
                }}
                onTogglePause={() => {
                  if (!selectedHostId) return;
                  const paused = sched.status === "paused";
                  void (paused
                    ? orbitIpc.resumeSchedule(selectedHostId, sched.schedule_id)
                    : orbitIpc.pauseSchedule(selectedHostId, sched.schedule_id));
                }}
                onEdit={() => openEditDialog(sched)}
                onRequestDelete={() => requestDeleteConfirm(sched.schedule_id)}
                onConfirmDelete={() => void confirmDelete(sched)}
                onCancelDelete={() => setConfirmDeleteId(null)}
              />
            ))}
          </div>
        )}

        {/* Live Agent Heartbeats — Unified Hairline List (kept alongside Schedules when present) */}
        {heartbeats.length > 0 && (
          <div className="space-y-2">
            <div className="text-muted-foreground/80 flex items-center gap-1.5 px-0.5 text-[11.5px] font-medium">
              <span>Agent Heartbeats</span>
              <span className="text-muted-foreground/60 tabular-nums">{heartbeats.length}</span>
            </div>
            <div className="border-border/80 bg-elevated/35 divide-border/70 divide-y overflow-hidden rounded-xl border">
              {heartbeats.map((hb) => {
                const targetAgentId =
                  hb.target.type === "existing_agent" ? hb.target.agent_id : null;
                const targetAgent = targetAgentId
                  ? hostDetail?.agents.find((a) => a.agent_id === targetAgentId)
                  : null;
                const isConfirming = confirmDeleteId === hb.schedule_id;
                return (
                  <div
                    key={hb.schedule_id}
                    className="group flex items-center justify-between gap-3 px-4 py-2.5 transition-colors hover:bg-black/[0.02] dark:hover:bg-white/[0.025]"
                  >
                    <div className="flex min-w-0 items-center gap-2.5">
                      <HeartPulse className="size-3.5 shrink-0 text-rose-500/80" />
                      <div className="min-w-0">
                        <div className="flex items-center gap-2">
                          <span className="text-foreground truncate text-[13px] font-medium">
                            {hb.name}
                          </span>
                          <span
                            title={hb.cron_expr}
                            className="text-muted-foreground/80 shrink-0 font-mono text-[11px]"
                          >
                            {formatHumanCadence(hb.cron_expr)}
                          </span>
                        </div>
                        <div className="text-muted-foreground/80 tabular-nums flex items-center gap-2 truncate text-[11.5px]">
                          <span className="truncate">
                            {targetAgent?.title ?? targetAgentId ?? "—"}
                          </span>
                          <span>·</span>
                          <span>Next {formatShortTime(hb.next_run_at)}</span>
                          <span>·</span>
                          <span>{hb.run_count} runs</span>
                        </div>
                      </div>
                    </div>

                    {selectedHostId && (
                      <div className="flex shrink-0 items-center gap-1">
                        {targetAgentId && !isConfirming && (
                          <button
                            type="button"
                            onClick={() => orbitActions.selectAgent(selectedHostId, targetAgentId)}
                            className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[26px] cursor-pointer items-center gap-1 rounded-md px-2 text-[11.5px] font-medium transition-colors"
                          >
                            <span>Open</span>
                            <ArrowUpRight className="size-3" />
                          </button>
                        )}
                        {isConfirming ? (
                          <div className="flex items-center gap-1">
                            <button
                              type="button"
                              onClick={() => void confirmDelete(hb)}
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
                          <SimpleTooltip label="Delete heartbeat">
                            <button
                              type="button"
                              aria-label="Delete heartbeat"
                              onClick={() => requestDeleteConfirm(hb.schedule_id)}
                              className="text-muted-foreground/75 hover:bg-accent inline-flex size-[26px] cursor-pointer items-center justify-center rounded-md transition-colors hover:text-rose-500"
                            >
                              <Trash2 className="size-3.5 stroke-[1.75]" />
                            </button>
                          </SimpleTooltip>
                        )}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          </div>
        )}
          </>
        ) : (
          /* Live Runs Tab: One-Shot Conditional Timers, Autonomous Goals & Streaming Monitors */
          <div className="space-y-5">
            {/* One-Shot Conditional Timers — Active countdowns & recent history */}
            <div className="space-y-2">
              <div className="flex items-center justify-between gap-2 px-0.5">
                <div className="text-muted-foreground/80 flex items-center gap-1.5 text-[11.5px] font-medium">
                  <span>Timers</span>
                  {armedTimerCount > 0 && (
                    <span className="text-muted-foreground/60 tabular-nums">
                      {armedTimerCount} armed
                    </span>
                  )}
                </div>
                <button
                  type="button"
                  disabled={!selectedHostId || activeAgentChoices.length === 0}
                  onClick={openTimerDialog}
                  className="border-border/80 bg-elevated/50 hover:bg-accent text-foreground/85 inline-flex h-[26px] cursor-pointer items-center gap-1 rounded-[7px] border px-2 text-[11.5px] font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-40"
                >
                  <Plus className="size-3 stroke-[2]" />
                  <span>One-Shot Timer</span>
                </button>
              </div>

              {conditionalTimers.length === 0 ? (
                <div className="border-border/65 text-muted-foreground/75 rounded-xl border border-dashed px-4 py-3 text-[12px]">
                  No active or recent one-shot conditional timers.
                </div>
              ) : (
                <div className="border-border/80 bg-elevated/35 divide-border/70 divide-y overflow-hidden rounded-xl border">
                  {conditionalTimers.slice(0, 20).map((timer: ConditionalTimerRecord) => {
                    const isArmed = timer.state === "armed";
                    const targetAgent = hostDetail?.agents.find((a) => a.agent_id === timer.agent_id);
                    const stateLabel =
                      timer.state === "fired"
                        ? "Fired"
                        : timer.state === "canceled_early"
                          ? "Canceled early"
                          : timer.state === "canceled_manual"
                            ? "Canceled"
                            : "Armed";
                    const busy = timerBusyId === timer.timer_id;
                    return (
                      <div
                        key={timer.timer_id}
                        className="group flex items-center justify-between gap-3 px-4 py-2.5 transition-colors hover:bg-black/[0.02] dark:hover:bg-white/[0.025]"
                      >
                        <div className="flex min-w-0 items-start gap-2.5">
                          <Timer
                            className={cn(
                              "mt-0.5 size-3.5 shrink-0",
                              isArmed ? "text-amber-500/85" : "text-muted-foreground/55",
                            )}
                          />
                          <div className="min-w-0 space-y-0.5">
                            <div className="flex flex-wrap items-center gap-1.5">
                              <span className="text-foreground max-w-[320px] truncate text-[12.5px] font-medium">
                                {timer.prompt}
                              </span>
                              {isArmed ? (
                                <span className="text-foreground/90 tabular-nums shrink-0 rounded-md bg-amber-500/12 px-1.5 py-0.5 font-mono text-[11px] font-medium">
                                  {formatTimerCountdown(timer.fire_at_ms, timerNowMs)}
                                </span>
                              ) : (
                                <span className="text-muted-foreground bg-muted/70 shrink-0 rounded-md px-1.5 py-0.5 text-[10.5px] font-medium">
                                  {stateLabel}
                                </span>
                              )}
                              <span className="text-muted-foreground/80 border-border/70 shrink-0 rounded-md border px-1.5 py-0.5 text-[10.5px]">
                                {formatTimerConditionLabel(timer.condition)}
                              </span>
                            </div>
                            <div className="text-muted-foreground/80 flex flex-wrap items-center gap-1.5 text-[11.5px]">
                              <span className="max-w-[200px] truncate">
                                {targetAgent?.title ?? timer.agent_id}
                              </span>
                              {timer.resolved_reason && (
                                <>
                                  <span>·</span>
                                  <span className="truncate">{timer.resolved_reason}</span>
                                </>
                              )}
                            </div>
                          </div>
                        </div>

                        {selectedHostId && (
                          <div className="flex shrink-0 items-center gap-1">
                            <button
                              type="button"
                              onClick={() => orbitActions.selectAgent(selectedHostId, timer.agent_id)}
                              className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[26px] cursor-pointer items-center gap-1 rounded-md px-2 text-[11.5px] font-medium transition-colors"
                            >
                              <span>Open</span>
                              <ArrowUpRight className="size-3" />
                            </button>
                            {isArmed && (
                              <>
                                <SimpleTooltip label="Run now">
                                  <button
                                    type="button"
                                    disabled={busy}
                                    aria-label="Run timer now"
                                    onClick={() => {
                                      setTimerBusyId(timer.timer_id);
                                      void orbitActions
                                        .fireConditionalTimerNow(selectedHostId, timer.timer_id)
                                        .then(
                                          () => orbitActions.showToast("Timer triggered"),
                                          (err) =>
                                            orbitActions.showToast(
                                              `Failed to trigger timer: ${String(err)}`,
                                            ),
                                        )
                                        .finally(() => setTimerBusyId(null));
                                    }}
                                    className="text-muted-foreground/75 hover:text-foreground hover:bg-accent inline-flex size-[26px] cursor-pointer items-center justify-center rounded-md transition-colors disabled:opacity-40"
                                  >
                                    <Play className="size-3 fill-current" />
                                  </button>
                                </SimpleTooltip>
                                <SimpleTooltip label="Cancel timer">
                                  <button
                                    type="button"
                                    disabled={busy}
                                    aria-label="Cancel timer"
                                    onClick={() => {
                                      setTimerBusyId(timer.timer_id);
                                      void orbitActions
                                        .cancelConditionalTimer(selectedHostId, timer.timer_id)
                                        .then(
                                          () => orbitActions.showToast("Timer canceled"),
                                          (err) =>
                                            orbitActions.showToast(
                                              `Failed to cancel timer: ${String(err)}`,
                                            ),
                                        )
                                        .finally(() => setTimerBusyId(null));
                                    }}
                                    className="text-muted-foreground/75 hover:bg-accent inline-flex size-[26px] cursor-pointer items-center justify-center rounded-md transition-colors hover:text-rose-500 disabled:opacity-40"
                                  >
                                    <X className="size-3.5 stroke-[1.8]" />
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
            </div>

            {goalRuns.length === 0 && monitorWatches.length === 0 && conditionalTimers.length === 0 && (
              <div className="flex flex-col items-center justify-center px-4 py-8 text-center">
                <p className="text-muted-foreground max-w-md text-[12.5px] leading-relaxed">
                  Autonomous <span className="text-foreground/85 font-mono">/goal</span> loops,
                  streaming background monitors, and conditional timers appear here while active.
                </p>
              </div>
            )}
        {/* Autonomous Goals & Streaming Monitors */}
        {(goalRuns.length > 0 || monitorWatches.length > 0) && (
          <div className="space-y-4">
            {goalRuns.length > 0 && (
              <div className="space-y-2">
                <div className="text-muted-foreground/80 flex items-center gap-1.5 px-0.5 text-[11.5px] font-medium">
                  <span>Autonomous Goals</span>
                  <span className="text-muted-foreground/60 tabular-nums">{goalRuns.length}</span>
                </div>
                <div className="border-border/80 bg-elevated/35 divide-border/70 divide-y overflow-hidden rounded-xl border">
                  {goalRuns.map((goal) => {
                    const targetAgent = hostDetail?.agents.find(
                      (a) => a.agent_id === goal.agent_id,
                    );
                    const isActive =
                      goal.status === "running" ||
                      goal.status === "verifying" ||
                      goal.status === "paused";
                    const isPaused = goal.status === "paused";
                    const isVerifying = goal.status === "verifying";
                    const dotClass =
                      goal.status === "running"
                        ? "bg-emerald-500 animate-pulse"
                        : goal.status === "verifying"
                          ? "bg-sky-500 animate-pulse"
                          : goal.status === "paused"
                            ? "bg-amber-500"
                            : goal.status === "succeeded"
                              ? "bg-emerald-500"
                              : goal.status === "failed"
                                ? "bg-rose-500"
                                : "bg-muted-foreground/45";
                    return (
                      <div
                        key={goal.goal_id}
                        className="group flex items-center justify-between gap-3 px-4 py-2.5 transition-colors hover:bg-black/[0.02] dark:hover:bg-white/[0.025]"
                      >
                        <div className="flex min-w-0 items-start gap-2.5">
                          <Target className="text-muted-foreground mt-0.5 size-3.5 shrink-0 stroke-[1.75]" />
                          <div className="min-w-0">
                            <div className="flex items-center gap-2">
                              <span
                                title={goal.status}
                                className={cn("size-1.5 shrink-0 rounded-full", dotClass)}
                              />
                              <span className="text-foreground truncate text-[13px] font-medium">
                                {goal.objective}
                              </span>
                              <span className="text-muted-foreground/80 tabular-nums shrink-0 font-mono text-[11px]">
                                Iter {goal.current_iteration}/{goal.max_iterations}
                              </span>
                            </div>
                            <div className="text-muted-foreground/75 flex items-center gap-2 truncate text-[11.5px]">
                              <span className="capitalize">{goal.status}</span>
                              {targetAgent && (
                                <>
                                  <span>·</span>
                                  <span className="truncate">{targetAgent.title}</span>
                                </>
                              )}
                              {goal.verify_command && (
                                <>
                                  <span>·</span>
                                  <span className="truncate font-mono text-[11px]">
                                    {goal.verify_command}
                                  </span>
                                </>
                              )}
                            </div>
                          </div>
                        </div>

                        {selectedHostId && (
                          <div className="flex shrink-0 items-center gap-0.5">
                            <button
                              type="button"
                              onClick={() =>
                                orbitActions.selectAgent(selectedHostId, goal.agent_id)
                              }
                              className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[26px] cursor-pointer items-center gap-1 rounded-md px-2 text-[11.5px] font-medium transition-colors"
                            >
                              <span>Open</span>
                              <ArrowUpRight className="size-3" />
                            </button>
                            {isActive && !isVerifying && (
                              <SimpleTooltip label="Verify now">
                                <button
                                  type="button"
                                  aria-label="Verify goal"
                                  onClick={() =>
                                    void orbitActions.controlGoalRun(
                                      selectedHostId,
                                      goal.goal_id,
                                      "verify_now",
                                    )
                                  }
                                  className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex size-[26px] cursor-pointer items-center justify-center rounded-md transition-colors"
                                >
                                  <CheckCircle2 className="size-3.5 stroke-[1.75]" />
                                </button>
                              </SimpleTooltip>
                            )}
                            {isActive && !isVerifying && (
                              <SimpleTooltip label={isPaused ? "Resume" : "Pause"}>
                                <button
                                  type="button"
                                  aria-label={isPaused ? "Resume goal" : "Pause goal"}
                                  onClick={() =>
                                    void orbitActions.controlGoalRun(
                                      selectedHostId,
                                      goal.goal_id,
                                      isPaused ? "resume" : "pause",
                                    )
                                  }
                                  className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex size-[26px] cursor-pointer items-center justify-center rounded-md transition-colors"
                                >
                                  {isPaused ? (
                                    <Play className="size-3.5 stroke-[1.75]" />
                                  ) : (
                                    <Pause className="size-3.5 stroke-[1.75]" />
                                  )}
                                </button>
                              </SimpleTooltip>
                            )}
                            {isActive && (
                              <SimpleTooltip label="Stop">
                                <button
                                  type="button"
                                  aria-label="Stop goal"
                                  onClick={() =>
                                    void orbitActions.controlGoalRun(
                                      selectedHostId,
                                      goal.goal_id,
                                      "cancel",
                                    )
                                  }
                                  className="text-muted-foreground hover:bg-accent inline-flex size-[26px] cursor-pointer items-center justify-center rounded-md transition-colors hover:text-rose-500"
                                >
                                  <Square className="size-3.5 stroke-[1.75]" />
                                </button>
                              </SimpleTooltip>
                            )}
                          </div>
                        )}
                      </div>
                    );
                  })}
                </div>
              </div>
            )}

            {monitorWatches.length > 0 && (
              <div className="space-y-2">
                <div className="text-muted-foreground/80 flex items-center gap-1.5 px-0.5 text-[11.5px] font-medium">
                  <span>Streaming Monitors</span>
                  <span className="text-muted-foreground/60 tabular-nums">
                    {monitorWatches.length}
                  </span>
                </div>
                <div className="border-border/80 bg-elevated/35 divide-border/70 divide-y overflow-hidden rounded-xl border">
                  {monitorWatches.map((mon) => {
                    const isRunning = mon.status === "active" || mon.status === "triggered";
                    const dotClass =
                      mon.status === "active"
                        ? "bg-emerald-500 animate-pulse"
                        : mon.status === "triggered"
                          ? "bg-amber-500 animate-pulse"
                          : mon.status === "error"
                            ? "bg-rose-500"
                            : "bg-muted-foreground/45";
                    const kindDetail =
                      mon.kind.kind === "command_stream"
                        ? `cmd: ${mon.kind.command}`
                        : mon.kind.kind === "log_file_tail"
                          ? `tail: ${mon.kind.path}`
                          : `http: ${mon.kind.url}`;
                    return (
                      <div
                        key={mon.monitor_id}
                        className="group flex items-center justify-between gap-3 px-4 py-2.5 transition-colors hover:bg-black/[0.02] dark:hover:bg-white/[0.025]"
                      >
                        <div className="flex min-w-0 items-start gap-2.5">
                          <Activity className="text-muted-foreground mt-0.5 size-3.5 shrink-0 stroke-[1.75]" />
                          <div className="min-w-0">
                            <div className="flex items-center gap-2">
                              <span
                                title={mon.status}
                                className={cn("size-1.5 shrink-0 rounded-full", dotClass)}
                              />
                              <span className="text-foreground truncate text-[13px] font-medium">
                                {mon.name}
                              </span>
                              <span className="text-muted-foreground/75 tabular-nums shrink-0 font-mono text-[11px]">
                                {mon.match_count} {mon.match_count === 1 ? "match" : "matches"}
                              </span>
                            </div>
                            <div className="text-muted-foreground/75 flex items-center gap-2 truncate text-[11.5px]">
                              <span className="truncate font-mono text-[11px]">{kindDetail}</span>
                              {mon.last_event_preview && (
                                <>
                                  <span>·</span>
                                  <span className="truncate">{mon.last_event_preview}</span>
                                </>
                              )}
                            </div>
                          </div>
                        </div>

                        {selectedHostId && (
                          <div className="flex shrink-0 items-center gap-0.5">
                            <button
                              type="button"
                              onClick={() =>
                                orbitActions.selectAgent(selectedHostId, mon.agent_id)
                              }
                              className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[26px] cursor-pointer items-center gap-1 rounded-md px-2 text-[11.5px] font-medium transition-colors"
                            >
                              <span>Open</span>
                              <ArrowUpRight className="size-3" />
                            </button>
                            {isRunning && (
                              <SimpleTooltip label="Stop monitor">
                                <button
                                  type="button"
                                  aria-label="Stop monitor"
                                  onClick={() =>
                                    void orbitActions.stopMonitorWatch(
                                      selectedHostId,
                                      mon.monitor_id,
                                    )
                                  }
                                  className="text-muted-foreground hover:bg-accent inline-flex size-[26px] cursor-pointer items-center justify-center rounded-md transition-colors hover:text-rose-500"
                                >
                                  <Square className="size-3.5 stroke-[1.75]" />
                                </button>
                              </SimpleTooltip>
                            )}
                          </div>
                        )}
                      </div>
                    );
                  })}
                </div>
              </div>
            )}
          </div>
        )}
          </div>
        )}
      </div>

      {/* One-Shot Conditional Timer Dialog */}
      <Dialog open={timerDialogOpen} onOpenChange={setTimerDialogOpen}>
        <DialogContent className="max-w-[440px] gap-0 p-0">
          <DialogHeader className="px-5 pt-5 pb-3">
            <DialogTitle className="text-[15px]">New one-shot timer</DialogTitle>
            <DialogDescription className="text-[12px]">
              Wake an agent after a delay unless an early-cancel condition triggers first.
            </DialogDescription>
          </DialogHeader>

          <div className="flex flex-col gap-3.5 px-5 pb-4">
            <label className="flex flex-col gap-1.5 text-[12px]">
              <span className="text-muted-foreground font-medium">Agent</span>
              <SettingSelect
                value={timerAgentId}
                onChange={setTimerAgentId}
                options={activeAgentChoices.map((a) => ({
                  value: a.agent_id,
                  label: a.title || a.agent_id,
                  description: shortWorkspaceName(a.cwd),
                }))}
                placeholder="Select agent"
              />
            </label>

            <div className="flex flex-col gap-1.5 text-[12px]">
              <span className="text-muted-foreground font-medium">Delay (seconds)</span>
              <div className="flex items-center gap-1.5">
                {[
                  { label: "1m", secs: "60" },
                  { label: "5m", secs: "300" },
                  { label: "15m", secs: "900" },
                  { label: "30m", secs: "1800" },
                  { label: "1h", secs: "3600" },
                ].map((preset) => (
                  <button
                    key={preset.secs}
                    type="button"
                    onClick={() => setTimerDurationSec(preset.secs)}
                    className={cn(
                      "h-[28px] cursor-pointer rounded-md border px-2 text-[11.5px] font-medium transition-colors",
                      timerDurationSec === preset.secs
                        ? "border-foreground/35 bg-accent text-foreground"
                        : "border-border/75 text-muted-foreground hover:text-foreground",
                    )}
                  >
                    {preset.label}
                  </button>
                ))}
                <input
                  type="number"
                  min={1}
                  max={86400}
                  value={timerDurationSec}
                  onChange={(e) => setTimerDurationSec(e.target.value)}
                  className="border-border bg-canvas text-foreground focus:border-foreground/30 ml-auto h-[28px] w-[84px] rounded-md border px-2 font-mono text-[12px] outline-none"
                />
              </div>
            </div>

            <label className="flex flex-col gap-1.5 text-[12px]">
              <span className="text-muted-foreground font-medium">Early-cancel condition</span>
              <SettingSelect
                value={timerConditionKind}
                onChange={(val) => {
                  setTimerConditionKind(val);
                  setTimerConditionArg("");
                }}
                options={[
                  { value: "any_message", label: "Any incoming message" },
                  { value: "all_subagents_settled", label: "All subagents settled" },
                  { value: "all_peers_settled", label: "All workspace peers settled" },
                  { value: "sender", label: "Message from specific sender" },
                  { value: "file_changed", label: "Workspace file changed" },
                  { value: "never", label: "Never (fixed timer)" },
                ]}
              />
            </label>

            {(timerConditionKind === "sender" || timerConditionKind === "file_changed") && (
              <label className="flex flex-col gap-1.5 text-[12px]">
                <span className="text-muted-foreground font-medium">
                  {timerConditionKind === "sender" ? "Sender Agent ID" : "Relative Workspace Path"}
                </span>
                <input
                  value={timerConditionArg}
                  onChange={(e) => setTimerConditionArg(e.target.value)}
                  placeholder={
                    timerConditionKind === "sender" ? "e.g. ag_12345678" : "e.g. dist/build.log"
                  }
                  className="border-border bg-canvas text-foreground placeholder:text-muted-foreground/50 focus:border-foreground/30 h-[34px] w-full rounded-lg border px-3 font-mono text-[12px] outline-none"
                />
              </label>
            )}

            <label className="flex flex-col gap-1.5 text-[12px]">
              <span className="text-muted-foreground font-medium">Wakeup prompt</span>
              <textarea
                rows={3}
                value={timerPrompt}
                onChange={(e) => setTimerPrompt(e.target.value)}
                placeholder="What should the agent check if this timer elapses?"
                className="border-border bg-canvas text-foreground placeholder:text-muted-foreground/50 focus:border-foreground/30 w-full resize-none rounded-lg border px-3 py-2 text-[12.5px] outline-none"
              />
            </label>
          </div>

          <div className="border-border/70 flex items-center justify-end gap-2 border-t px-5 py-3">
            <button
              type="button"
              onClick={() => setTimerDialogOpen(false)}
              className="text-muted-foreground hover:text-foreground h-[30px] cursor-pointer rounded-lg px-3 text-[12.5px] font-medium transition-colors"
            >
              Cancel
            </button>
            <button
              type="button"
              disabled={timerSubmitting}
              onClick={() => void submitConditionalTimer()}
              className="bg-foreground text-background hover:opacity-90 inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg px-3.5 text-[12.5px] font-medium transition-opacity disabled:opacity-40"
            >
              {timerSubmitting ? "Arming…" : "Arm timer"}
            </button>
          </div>
        </DialogContent>
      </Dialog>

      {/* Unified Create / Edit Automation Dialog */}
      <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
        <DialogContent className="max-w-[480px] gap-0 p-0">
          <DialogHeader className="px-5 pt-5 pb-3">
            <DialogTitle className="text-[15px]">
              {editingSchedule ? "Edit automation" : "New automation"}
            </DialogTitle>
            <DialogDescription className={showHostContext ? "text-[12px]" : "sr-only"}>
              {editingSchedule
                ? "Update automation schedule and instructions."
                : showHostContext
                  ? `Runs in the background on ${activeHostLabel}.`
                  : "Create a scheduled background automation."}
            </DialogDescription>
          </DialogHeader>

          <ScrollArea viewportClassName="max-h-[min(70vh,560px)] px-5 pb-4">
            <div className="flex flex-col gap-3.5">
              {/* Primary 1: Name (optional — auto-derived from prompt if blank) */}
              <label className="flex flex-col gap-1.5 text-[12px]">
                <span className="text-muted-foreground font-medium">Name</span>
                <input
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder="Optional — auto-named from prompt"
                  className="border-border bg-canvas text-foreground placeholder:text-muted-foreground/50 focus:border-foreground/30 h-[34px] w-full rounded-lg border px-3 text-[13px] transition-colors outline-none"
                />
              </label>

              {/* Primary 2: Prompt */}
              <label className="flex flex-col gap-1.5 text-[12px]">
                <span className="text-muted-foreground font-medium">Prompt</span>
                <textarea
                  autoFocus
                  rows={4}
                  value={prompt}
                  onChange={(e) => setPrompt(e.target.value)}
                  onKeyDown={(e) => {
                    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
                      e.preventDefault();
                      void handleSubmitDialog();
                    }
                  }}
                  placeholder="Describe what the agent should check or run each time…"
                  className="border-border bg-canvas text-foreground placeholder:text-muted-foreground/50 focus:border-foreground/30 w-full resize-none rounded-lg border px-3 py-2.5 text-[13px] leading-relaxed transition-colors outline-none"
                />
              </label>

              {/* Primary 3 & 4: Project + Schedule */}
              <div
                className={cn(
                  "grid grid-cols-1 gap-3",
                  !editingSchedule && projectSelectOptions.length > 0 && "sm:grid-cols-2",
                )}
              >
                {!editingSchedule && projectSelectOptions.length > 0 && (
                  <div className="flex flex-col gap-1.5 text-[12px]">
                    <span className="text-muted-foreground font-medium">Project</span>
                    <SettingSelect
                      value={effectiveProjectPath}
                      options={projectSelectOptions}
                      onChange={(val) => {
                        setTargetProjectPath(val);
                        setWorktreeRepoPath("");
                      }}
                      triggerClassName="w-full h-[34px] min-w-0"
                      menuClassName="w-[260px]"
                    />
                  </div>
                )}

                <div className="flex flex-col gap-1.5 text-[12px]">
                  <span className="text-muted-foreground font-medium">Schedule</span>
                  <SettingSelect
                    value={schedulePreset}
                    options={SCHEDULE_PRESET_OPTIONS}
                    onChange={(val) => {
                      setSchedulePreset(val);
                      if (val !== CUSTOM_CRON_VALUE) {
                        setCustomCronExpr(val);
                      }
                    }}
                    triggerClassName="w-full h-[34px] min-w-0"
                    menuClassName="w-52"
                  />
                </div>
              </div>

              {/* Conditional Custom Cron input (only shown when "Custom cron…" is picked) */}
              {schedulePreset === CUSTOM_CRON_VALUE && (
                <label className="flex flex-col gap-1.5 text-[12px]">
                  <span className="text-muted-foreground flex items-center justify-between font-medium">
                    <span>Cron expression</span>
                    <span className="text-muted-foreground/70 font-normal">
                      {humanCronPreview}
                    </span>
                  </span>
                  <input
                    value={customCronExpr}
                    onChange={(e) => setCustomCronExpr(e.target.value)}
                    placeholder="*/30 * * * * or daily 09:30"
                    className="border-border bg-canvas text-foreground placeholder:text-muted-foreground/50 focus:border-foreground/30 h-[34px] w-full rounded-lg border px-3 font-mono text-[12px] transition-colors outline-none"
                  />
                </label>
              )}

              {/* Progressive Disclosure: Single-line Advanced summary */}
              <div className="pt-0.5">
                <button
                  type="button"
                  onClick={() => setAdvancedOpen((v) => !v)}
                  className="text-muted-foreground hover:text-foreground flex w-full cursor-pointer items-center justify-between gap-3 rounded-lg py-1.5 text-left transition-colors"
                >
                  <span className="inline-flex shrink-0 items-center gap-1.5 text-[12px] font-medium">
                    <ChevronRight
                      className={cn(
                        "size-3.5 transition-transform duration-150",
                        advancedOpen && "rotate-90",
                      )}
                    />
                    <span>Advanced</span>
                  </span>
                  {!advancedOpen && (
                    <span className="text-muted-foreground/65 min-w-0 truncate text-[11.5px]">
                      {advancedSummary}
                    </span>
                  )}
                </button>

                {advancedOpen && (
                  <div className="border-border/70 bg-canvas/45 mt-1.5 flex flex-col gap-3 rounded-xl border p-3">
                    {!editingSchedule && (
                      <div
                        className={cn(
                          "grid grid-cols-1 gap-2.5",
                          profiles.length > 0 ? "sm:grid-cols-3" : "sm:grid-cols-2",
                        )}
                      >
                        <div className="flex flex-col gap-1 text-[11.5px]">
                          <span className="text-muted-foreground font-medium">Provider</span>
                          <SettingSelect
                            value={provider}
                            options={providerSelectOptions}
                            onChange={setProvider}
                            triggerClassName="w-full h-[30px] min-w-0"
                            menuClassName="w-44"
                          />
                        </div>

                        {profiles.length > 0 && (
                          <div className="flex flex-col gap-1 text-[11.5px]">
                            <span className="text-muted-foreground font-medium">Profile</span>
                            <SettingSelect
                              value={profileId}
                              options={profileSelectOptions}
                              onChange={setProfileId}
                              triggerClassName="w-full h-[30px] min-w-0"
                              menuClassName="w-48"
                            />
                          </div>
                        )}

                        <div className="flex flex-col gap-1 text-[11.5px]">
                          <span className="text-muted-foreground font-medium">Permissions</span>
                          <SettingSelect
                            value={mode}
                            options={PERMISSION_OPTIONS}
                            onChange={setMode}
                            triggerClassName="w-full h-[30px] min-w-0"
                            menuClassName="w-52"
                          />
                        </div>
                      </div>
                    )}

                    <div
                      className={cn(
                        "divide-border/60 divide-y",
                        !editingSchedule && "border-border/60 border-t pt-1",
                      )}
                    >
                      {!editingSchedule && (
                        <div className="flex items-center justify-between gap-3 py-2">
                          <div className="min-w-0">
                            <div className="text-foreground text-[12px] font-medium">
                              Isolated Git worktree
                            </div>
                            <div className="text-muted-foreground text-[11px]">
                              {canUseWorktree
                                ? "Run each trigger in a clean worktree branch"
                                : "Selected folder is not a Git repository"}
                            </div>
                          </div>
                          <div className={cn(!canUseWorktree && "pointer-events-none opacity-45")}>
                            <ToggleSwitch checked={effectiveWorktree} onChange={setWorktree} />
                          </div>
                        </div>
                      )}

                      {!editingSchedule &&
                        effectiveWorktree &&
                        !isDirectGitRepo &&
                        nestedRepoSelectOptions.length > 0 && (
                          <div className="flex items-center justify-between gap-3 py-2">
                            <span className="text-muted-foreground text-[12px]">Worktree repo</span>
                            <SettingSelect
                              value={worktreeRepoPath || nestedRepoSelectOptions[0].value}
                              options={nestedRepoSelectOptions}
                              onChange={setWorktreeRepoPath}
                              triggerClassName="h-[28px] min-w-[150px]"
                              menuClassName="w-56"
                            />
                          </div>
                        )}

                      <div className="flex items-center justify-between gap-3 pt-2">
                        <div className="min-w-0">
                          <div className="text-foreground text-[12px] font-medium">
                            Auto-archive finished runs
                          </div>
                          <div className="text-muted-foreground text-[11px]">
                            Keep the main sidebar clean after each background run
                          </div>
                        </div>
                        <ToggleSwitch checked={archiveOnFinish} onChange={setArchiveOnFinish} />
                      </div>
                    </div>
                  </div>
                )}
              </div>
            </div>
          </ScrollArea>

          <div className="border-border/70 flex items-center justify-end gap-2 border-t px-5 py-3">
            <button
              type="button"
              onClick={() => setDialogOpen(false)}
              className="border-border text-foreground/80 hover:bg-accent hover:text-foreground h-[30px] cursor-pointer rounded-lg border px-3 text-[12.5px] font-medium transition-colors"
            >
              Cancel
            </button>
            <button
              type="button"
              disabled={submitting || !effectiveCronExpr || !prompt.trim()}
              onClick={() => void handleSubmitDialog()}
              className="bg-foreground text-background hover:opacity-90 inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg px-3.5 text-[12.5px] font-medium transition-opacity disabled:cursor-not-allowed disabled:opacity-45"
            >
              {submitting && <OrbitSpinner size={12} />}
              <span>{editingSchedule ? "Save" : "Create"}</span>
            </button>
          </div>
        </DialogContent>
      </Dialog>
    </ScrollArea>
  );
}

const ScheduleListRow = memo(function ScheduleListRow({
  sched,
  selectedHostId,
  isExpanded,
  isLoadingRuns,
  runs,
  isConfirmingDelete,
  onToggleExpand,
  onRefreshRuns,
  onRunNow,
  onTogglePause,
  onEdit,
  onRequestDelete,
  onConfirmDelete,
  onCancelDelete,
}: {
  sched: ScheduleSpec;
  selectedHostId: string | null;
  isExpanded: boolean;
  isLoadingRuns: boolean;
  runs: ScheduleRunRecord[] | undefined;
  isConfirmingDelete: boolean;
  onToggleExpand: () => void;
  onRefreshRuns: () => void;
  onRunNow: () => void;
  onTogglePause: () => void;
  onEdit: () => void;
  onRequestDelete: () => void;
  onConfirmDelete: () => void;
  onCancelDelete: () => void;
}) {
  const isPaused = sched.status === "paused";
  const isCompleted = sched.status === "completed";
  const targetCfg = sched.target.type === "new_agent" ? sched.target : null;
  const projectLabel = targetCfg ? shortWorkspaceName(targetCfg.cwd) : null;
  const cadenceLabel = formatHumanCadence(sched.cron_expr);

  const statusDotClass =
    sched.status === "active"
      ? "bg-emerald-500"
      : isPaused
        ? "bg-amber-500/85"
        : "bg-muted-foreground/45";

  return (
    <div>
      <div
        role="button"
        tabIndex={0}
        onClick={onToggleExpand}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onToggleExpand();
          }
        }}
        className="group flex cursor-pointer items-center justify-between gap-4 px-4 py-3 transition-colors outline-none hover:bg-black/[0.02] focus-visible:bg-black/[0.03] dark:hover:bg-white/[0.025] dark:focus-visible:bg-white/[0.04]"
      >
        <div className="flex min-w-0 flex-1 items-start gap-2.5">
          <ChevronRight
            className={cn(
              "text-muted-foreground/55 mt-[3px] size-3.5 shrink-0 transition-transform duration-150",
              isExpanded && "rotate-90 text-foreground/75",
            )}
          />
          <div className="min-w-0 flex-1 space-y-1">
            <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
              <span
                title={sched.status}
                className={cn("size-1.5 shrink-0 rounded-full", statusDotClass)}
              />
              <span
                className={cn(
                  "truncate text-[13.5px] font-medium",
                  isPaused || isCompleted ? "text-foreground/65" : "text-foreground",
                )}
              >
                {sched.name}
              </span>
              <span
                title={sched.cron_expr}
                className="text-muted-foreground/85 shrink-0 font-mono text-[11px]"
              >
                {cadenceLabel}
              </span>
              {projectLabel && (
                <span
                  title={targetCfg?.cwd}
                  className="text-muted-foreground/75 inline-flex max-w-36 items-center gap-1 truncate text-[11.5px]"
                >
                  <Folder className="size-3 shrink-0 opacity-75" />
                  <span className="truncate">{projectLabel}</span>
                </span>
              )}
              {targetCfg?.worktree && (
                <SimpleTooltip label="Isolated Git worktree">
                  <span className="text-muted-foreground/65 inline-flex items-center">
                    <GitBranch className="size-3 shrink-0" />
                  </span>
                </SimpleTooltip>
              )}
            </div>

            <p className="text-muted-foreground/80 truncate text-[12px] leading-snug">
              {sched.prompt}
            </p>
          </div>
        </div>

        {/* Right: Quiet timing + Hover-revealed borderless actions */}
        <div
          className="flex shrink-0 items-center gap-3"
          onClick={(e) => e.stopPropagation()}
        >
          {!isConfirmingDelete && (
            <div className="text-muted-foreground/75 tabular-nums hidden text-right text-[11.5px] sm:block">
              <div>
                {isPaused
                  ? "Paused"
                  : isCompleted
                    ? "Completed"
                    : `Next ${formatShortTime(sched.next_run_at)}`}
              </div>
              <div className="text-muted-foreground/55 text-[11px]">
                {sched.run_count === 0
                  ? "Never run"
                  : `${sched.run_count} ${sched.run_count === 1 ? "run" : "runs"}`}
              </div>
            </div>
          )}

          {selectedHostId && (
            <div
              className={cn(
                "flex items-center gap-0.5 transition-opacity duration-150",
                !isExpanded &&
                  !isConfirmingDelete &&
                  "opacity-70 group-hover:opacity-100 focus-within:opacity-100",
              )}
            >
              {isConfirmingDelete ? (
                <div className="flex items-center gap-1">
                  <button
                    type="button"
                    onClick={onConfirmDelete}
                    className="cursor-pointer rounded-md bg-rose-500/12 px-2 py-1 text-[11.5px] font-medium text-rose-600 transition-colors hover:bg-rose-500/20 dark:text-rose-400"
                  >
                    Delete
                  </button>
                  <button
                    type="button"
                    onClick={onCancelDelete}
                    className="text-muted-foreground hover:text-foreground cursor-pointer rounded-md px-1.5 py-1 text-[11.5px]"
                  >
                    Cancel
                  </button>
                </div>
              ) : (
                <>
                  <SimpleTooltip label="Run now">
                    <button
                      type="button"
                      aria-label="Run now"
                      onClick={onRunNow}
                      className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex size-[26px] cursor-pointer items-center justify-center rounded-md transition-colors"
                    >
                      <Zap className="size-3.5 stroke-[1.75]" />
                    </button>
                  </SimpleTooltip>
                  {!isCompleted && (
                    <SimpleTooltip label={isPaused ? "Resume" : "Pause"}>
                      <button
                        type="button"
                        aria-label={isPaused ? "Resume" : "Pause"}
                        onClick={onTogglePause}
                        className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex size-[26px] cursor-pointer items-center justify-center rounded-md transition-colors"
                      >
                        {isPaused ? (
                          <Play className="size-3.5 stroke-[1.75]" />
                        ) : (
                          <Pause className="size-3.5 stroke-[1.75]" />
                        )}
                      </button>
                    </SimpleTooltip>
                  )}
                  <SimpleTooltip label="Edit">
                    <button
                      type="button"
                      aria-label="Edit automation"
                      onClick={onEdit}
                      className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex size-[26px] cursor-pointer items-center justify-center rounded-md transition-colors"
                    >
                      <Pencil className="size-3.5 stroke-[1.75]" />
                    </button>
                  </SimpleTooltip>
                  <SimpleTooltip label="Delete">
                    <button
                      type="button"
                      aria-label="Delete automation"
                      onClick={onRequestDelete}
                      className="text-muted-foreground hover:bg-accent inline-flex size-[26px] cursor-pointer items-center justify-center rounded-md transition-colors hover:text-rose-500"
                    >
                      <Trash2 className="size-3.5 stroke-[1.75]" />
                    </button>
                  </SimpleTooltip>
                </>
              )}
            </div>
          )}
        </div>
      </div>

      {/* Expanded Details & Run History */}
      {isExpanded && (
        <div className="border-border/60 bg-canvas/50 space-y-3 border-t px-4 py-3 pl-10">
          <p className="text-foreground/85 text-[12.5px] leading-relaxed whitespace-pre-wrap">
            {sched.prompt}
          </p>

          {targetCfg && (
            <div className="text-muted-foreground/80 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11.5px]">
              <span title={targetCfg.cwd} className="font-mono">
                {shortWorkspaceName(targetCfg.cwd)}
              </span>
              <span>·</span>
              <span>{formatProviderLabel(targetCfg.provider)}</span>
              <span>·</span>
              <span>{formatModeLabel(targetCfg.mode)}</span>
              <span>·</span>
              <span>{targetCfg.worktree ? "Worktree" : "Local"}</span>
              {targetCfg.archive_on_finish && (
                <>
                  <span>·</span>
                  <span>Auto-archive</span>
                </>
              )}
              <span>·</span>
              <span className="font-mono">{sched.cron_expr}</span>
            </div>
          )}

          <div className="border-border/60 space-y-1.5 border-t pt-2.5">
            <div className="flex items-center justify-between gap-2">
              <span className="text-muted-foreground/75 text-[11px] font-medium tracking-[0.02em] uppercase">
                Recent runs
              </span>
              <button
                type="button"
                disabled={isLoadingRuns}
                onClick={onRefreshRuns}
                className="text-muted-foreground hover:text-foreground inline-flex cursor-pointer items-center gap-1 rounded px-1.5 py-0.5 text-[11px] transition-colors disabled:opacity-50"
              >
                <RefreshCw className={cn("size-3", isLoadingRuns && "animate-spin")} />
                <span>Refresh</span>
              </button>
            </div>

            {isLoadingRuns && (!runs || runs.length === 0) ? (
              <div className="text-muted-foreground flex items-center gap-2 py-1 text-[12px]">
                <OrbitSpinner size={12} />
                <span>Loading run history…</span>
              </div>
            ) : !runs || runs.length === 0 ? (
              <div className="text-muted-foreground/75 py-1 text-[12px]">
                No executions recorded yet.
              </div>
            ) : (
              <div className="space-y-1">
                {runs.map((r) => {
                  const duration = formatRunDuration(r.started_at, r.finished_at);
                  const dotColor =
                    r.status === "succeeded"
                      ? "bg-emerald-500"
                      : r.status === "failed"
                        ? "bg-rose-500"
                        : r.status === "running"
                          ? "bg-sky-500 animate-pulse"
                          : "bg-muted-foreground/45";
                  const note = (r.error || r.summary || "").trim();
                  return (
                    <div
                      key={r.run_id}
                      className="flex items-center justify-between gap-3 py-0.5 text-[12px]"
                    >
                      <div className="flex min-w-0 flex-1 items-center gap-2">
                        <span className={cn("size-1.5 shrink-0 rounded-full", dotColor)} />
                        <span className="text-foreground/85 shrink-0 capitalize">{r.status}</span>
                        <span className="text-muted-foreground/75 tabular-nums shrink-0 text-[11.5px]">
                          {formatShortTime(r.started_at)}
                        </span>
                        {duration && (
                          <span className="text-muted-foreground/60 tabular-nums shrink-0 text-[11px]">
                            {duration}
                          </span>
                        )}
                        {note && (
                          <span
                            title={note}
                            className={cn(
                              "min-w-0 truncate text-[11.5px]",
                              r.error ? "text-rose-500/90" : "text-muted-foreground/75",
                            )}
                          >
                            — {note}
                          </span>
                        )}
                      </div>
                      {r.agent_id && selectedHostId && (
                        <button
                          type="button"
                          onClick={() => orbitActions.selectAgent(selectedHostId, r.agent_id!)}
                          className="text-muted-foreground hover:text-foreground inline-flex shrink-0 cursor-pointer items-center gap-0.5 rounded px-1.5 py-0.5 text-[11.5px] font-medium transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
                        >
                          <span>Open</span>
                          <ArrowUpRight className="size-3" />
                        </button>
                      )}
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
});
