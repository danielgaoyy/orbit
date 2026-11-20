import { useMemo, useState } from "react";
import {
  ArrowUpRight,
  Clock,
  FileText,
  GitCompareArrows,
  HeartPulse,
  History,
  ListTodo,
  Pause,
  Play,
  Plus,
  ShieldCheck,
  Trash2,
  Zap,
  type LucideIcon,
} from "lucide-react";
import {
  type ProviderKind,
  type ScheduleRunRecord,
  type ScheduleSpec,
  type SessionMode,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { ScrollArea } from "@/components/ui/scroll-area";
import { type SlashDirective, orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

interface AutomationTemplate {
  id: string;
  title: string;
  description: string;
  cadenceLabel: string;
  icon: LucideIcon;
  slash?: SlashDirective | null;
  prompt: string;
}

const TEMPLATES: AutomationTemplate[] = [
  {
    id: "diff-audit",
    title: "Working Tree Diff Audit",
    description:
      "Inspect uncommitted changes for regressions, edge cases, and architectural boundary violations.",
    cadenceLabel: "On demand · /review",
    icon: GitCompareArrows,
    slash: "review",
    prompt:
      "Audit the current working tree changes for correctness bugs, edge cases, and adherence to project conventions.",
  },
  {
    id: "repo-brief",
    title: "Daily Repository Brief",
    description:
      "Summarize recent commits, active branches, and unfinished work items across the current workspace.",
    cadenceLabel: "Routine digest",
    icon: FileText,
    prompt:
      "Inspect recent git commits, branch status, and uncommitted changes in this repository, then give me a concise engineering status brief and suggested next steps.",
  },
  {
    id: "quality-gate",
    title: "Build, Lint & Test Verification",
    description:
      "Run static checks and unit tests on the active host and diagnose any failing targets.",
    cadenceLabel: "Health check",
    icon: ShieldCheck,
    prompt:
      "Run the project's standard lint, typecheck, and test commands, and report any warnings or failures with concrete fixes.",
  },
  {
    id: "tech-debt-plan",
    title: "Refactor & Cleanup Plan",
    description:
      "Survey hotspots in the current module and propose an incremental, low-risk refactoring plan.",
    cadenceLabel: "Planning · /plan",
    icon: ListTodo,
    slash: "plan",
    prompt:
      "Survey the codebase for high-complexity modules or duplicated logic and draft a concise, incremental refactoring plan.",
  },
];

const CRON_PRESETS = [
  { label: "Every 15m", value: "15m" },
  { label: "Hourly", value: "1h" },
  { label: "Daily 09:00", value: "daily 09:00" },
  { label: "Weekdays 09:00", value: "weekdays 09:00" },
];

function shortWorkspaceName(cwd: string): string {
  if (!cwd || cwd === "~") return "~";
  const clean = cwd.replace(/\/+$/, "");
  const idx = clean.lastIndexOf("/");
  return idx >= 0 ? clean.slice(idx + 1) || "/" : clean;
}

function formatShortTime(iso: string | null): string {
  if (!iso) return "—";
  try {
    const d = new Date(iso);
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

export function AutomationsView() {
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const hostDetail = useOrbitSelector((s) =>
    s.selectedHostId ? (s.hostDetails[s.selectedHostId] ?? null) : null,
  );
  const draftProvider = useOrbitSelector((s) => s.draftProvider);

  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");
  const [cronExpr, setCronExpr] = useState("daily 09:00");
  const [prompt, setPrompt] = useState("");
  const [provider, setProvider] = useState<ProviderKind>(draftProvider);
  const [profileId, setProfileId] = useState<string>("");
  const [mode, setMode] = useState<SessionMode>("accept_edits");
  const [worktree, setWorktree] = useState(true);
  const [archiveOnFinish, setArchiveOnFinish] = useState(true);

  const [expandedRunsId, setExpandedRunsId] = useState<string | null>(null);
  const [runsBySchedule, setRunsBySchedule] = useState<Record<string, ScheduleRunRecord[]>>({});

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

  const handleToggleRuns = async (sched: ScheduleSpec) => {
    if (!selectedHostId) return;
    if (expandedRunsId === sched.schedule_id) {
      setExpandedRunsId(null);
      return;
    }
    setExpandedRunsId(sched.schedule_id);
    await orbitIpc.fetchScheduleRuns(selectedHostId, sched.schedule_id, 20);
    window.setTimeout(async () => {
      const runs = await orbitIpc.getScheduleRuns(selectedHostId, sched.schedule_id);
      setRunsBySchedule((prev) => ({ ...prev, [sched.schedule_id]: runs }));
    }, 180);
  };

  const handleCreateSchedule = async () => {
    if (!selectedHostId || !name.trim() || !cronExpr.trim() || !prompt.trim()) {
      orbitActions.showToast("Fill in name, schedule expression, and prompt");
      return;
    }
    await orbitIpc.createSchedule({
      hostId: selectedHostId,
      name: name.trim(),
      cronExpr: cronExpr.trim(),
      prompt: prompt.trim(),
      target: {
        type: "new_agent",
        cwd: selectedCwd || "~",
        provider,
        profile_id: profileId || null,
        mode,
        worktree,
        archive_on_finish: archiveOnFinish,
      },
    });
    setName("");
    setPrompt("");
    setCreating(false);
    orbitActions.showToast("Schedule created");
  };

  return (
    <ScrollArea className="orbit-stage-enter min-h-0 flex-1" viewportClassName="px-6 py-8">
      <div className="mx-auto w-full max-w-[780px] space-y-8">
        {/* Header */}
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div className="space-y-1.5">
            <div className="bg-muted text-foreground/75 mb-2 inline-flex h-[36px] w-[36px] items-center justify-center rounded-xl">
              <Clock className="h-[18px] w-[18px] stroke-[1.75]" />
            </div>
            <h1 className="text-foreground text-[18px] font-semibold tracking-[-0.015em]">
              Automations
            </h1>
            <p className="text-muted-foreground max-w-xl text-[13.5px] leading-relaxed">
              Recurring Cron Schedules, live Agent Heartbeats, and on-demand workflows on{" "}
              <span className="text-foreground font-medium">
                {hostDetail?.host.display_name ?? "selected host"}
              </span>
              {selectedCwd ? ` (${shortWorkspaceName(selectedCwd)})` : ""}.
            </p>
          </div>

          <button
            type="button"
            disabled={!selectedHostId}
            onClick={() => setCreating((v) => !v)}
            className="bg-foreground text-background hover:opacity-90 inline-flex h-[34px] cursor-pointer items-center gap-1.5 rounded-xl px-3.5 text-[13px] font-medium transition-opacity disabled:opacity-40"
          >
            <Plus className="h-[14px] w-[14px]" />
            <span>New Schedule</span>
          </button>
        </div>

        {/* Create Schedule Form */}
        {creating && selectedHostId && (
          <div className="border-border bg-elevated/80 space-y-4 rounded-2xl border p-4.5">
            <div className="text-foreground text-[14px] font-medium">New Recurring Schedule</div>
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <label className="space-y-1 text-[12px]">
                <span className="text-muted-foreground font-medium">Name</span>
                <input
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder="Nightly Test Suite Audit"
                  className="border-border bg-canvas text-foreground w-full rounded-lg border px-2.5 py-1.5 text-[13px] outline-none"
                />
              </label>

              <label className="space-y-1 text-[12px]">
                <span className="text-muted-foreground font-medium">
                  Cadence (5-field Cron or preset)
                </span>
                <input
                  value={cronExpr}
                  onChange={(e) => setCronExpr(e.target.value)}
                  placeholder="daily 09:00 or */30 * * * *"
                  className="border-border bg-canvas text-foreground w-full rounded-lg border px-2.5 py-1.5 font-mono text-[12.5px] outline-none"
                />
              </label>
            </div>

            <div className="flex flex-wrap gap-1.5">
              {CRON_PRESETS.map((p) => (
                <button
                  key={p.value}
                  type="button"
                  onClick={() => setCronExpr(p.value)}
                  className={cn(
                    "border-border hover:bg-accent cursor-pointer rounded-full border px-2.5 py-0.5 text-[11.5px] transition-colors",
                    cronExpr === p.value && "bg-accent text-foreground font-medium",
                  )}
                >
                  {p.label}
                </button>
              ))}
            </div>

            <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
              <label className="space-y-1 text-[12px]">
                <span className="text-muted-foreground font-medium">Provider</span>
                <select
                  value={provider}
                  onChange={(e) => setProvider(e.target.value as ProviderKind)}
                  className="border-border bg-canvas text-foreground w-full rounded-lg border px-2 py-1.5 text-[12.5px]"
                >
                  <option value="claude_code">Claude Code</option>
                  <option value="codex">Codex</option>
                  <option value="antigravity">Antigravity</option>
                </select>
              </label>

              <label className="space-y-1 text-[12px]">
                <span className="text-muted-foreground font-medium">Profile (optional)</span>
                <select
                  value={profileId}
                  onChange={(e) => setProfileId(e.target.value)}
                  className="border-border bg-canvas text-foreground w-full rounded-lg border px-2 py-1.5 text-[12.5px]"
                >
                  <option value="">None (use provider)</option>
                  {profiles.map((p) => (
                    <option key={p.profile_id} value={p.profile_id}>
                      {p.name}
                    </option>
                  ))}
                </select>
              </label>

              <label className="space-y-1 text-[12px]">
                <span className="text-muted-foreground font-medium">Permissions</span>
                <select
                  value={typeof mode === "string" ? mode : "accept_edits"}
                  onChange={(e) => setMode(e.target.value as SessionMode)}
                  className="border-border bg-canvas text-foreground w-full rounded-lg border px-2 py-1.5 text-[12.5px]"
                >
                  <option value="accept_edits">Accept Edits</option>
                  <option value="full_access">Full Access</option>
                  <option value="plan">Plan (Read-only)</option>
                  <option value="default">Ask</option>
                </select>
              </label>
            </div>

            <label className="block space-y-1 text-[12px]">
              <span className="text-muted-foreground font-medium">Prompt</span>
              <textarea
                rows={3}
                value={prompt}
                onChange={(e) => setPrompt(e.target.value)}
                placeholder="Run cargo test, inspect failures, and summarize findings..."
                className="border-border bg-canvas text-foreground w-full rounded-lg border p-2.5 text-[13px] outline-none"
              />
            </label>

            <div className="flex flex-wrap items-center justify-between gap-3">
              <div className="flex flex-wrap items-center gap-4 text-[12.5px]">
                <label className="inline-flex cursor-pointer items-center gap-1.5">
                  <input
                    type="checkbox"
                    checked={worktree}
                    onChange={(e) => setWorktree(e.target.checked)}
                  />
                  <span>Run in isolated git worktree</span>
                </label>
                <label className="inline-flex cursor-pointer items-center gap-1.5">
                  <input
                    type="checkbox"
                    checked={archiveOnFinish}
                    onChange={(e) => setArchiveOnFinish(e.target.checked)}
                  />
                  <span>Auto-archive run when finished</span>
                </label>
              </div>

              <div className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={() => setCreating(false)}
                  className="border-border hover:bg-accent cursor-pointer rounded-lg border px-3 py-1.5 text-[12.5px]"
                >
                  Cancel
                </button>
                <button
                  type="button"
                  onClick={() => void handleCreateSchedule()}
                  className="bg-foreground text-background cursor-pointer rounded-lg px-3.5 py-1.5 text-[12.5px] font-medium"
                >
                  Create Schedule
                </button>
              </div>
            </div>
          </div>
        )}

        {/* Active Cron Schedules */}
        <div className="space-y-2.5">
          <div className="text-muted-foreground px-0.5 text-[12.5px] font-medium">
            Recurring Schedules ({cronSchedules.length})
          </div>
          {cronSchedules.length === 0 ? (
            <div className="border-border text-muted-foreground rounded-2xl border border-dashed px-4 py-6 text-center text-[13px]">
              No recurring schedules configured on this host yet.
            </div>
          ) : (
            <div className="space-y-2.5">
              {cronSchedules.map((sched) => {
                const isPaused = sched.status === "paused";
                const isExpanded = expandedRunsId === sched.schedule_id;
                const runs = runsBySchedule[sched.schedule_id] ?? [];
                return (
                  <div
                    key={sched.schedule_id}
                    className="border-border bg-elevated/55 rounded-2xl border p-4"
                  >
                    <div className="flex flex-wrap items-start justify-between gap-3">
                      <div className="min-w-0 space-y-1">
                        <div className="flex items-center gap-2">
                          <span className="text-foreground font-medium text-[14px]">
                            {sched.name}
                          </span>
                          <span
                            className={cn(
                              "rounded-md px-1.5 py-0.5 text-[11px] font-medium",
                              sched.status === "active"
                                ? "bg-emerald-500/12 text-emerald-600 dark:text-emerald-400"
                                : sched.status === "paused"
                                  ? "bg-amber-500/12 text-amber-600 dark:text-amber-400"
                                  : "bg-muted text-muted-foreground",
                            )}
                          >
                            {sched.status}
                          </span>
                          <code className="bg-muted text-muted-foreground rounded px-1.5 py-0.5 text-[11px]">
                            {sched.cron_expr}
                          </code>
                        </div>
                        <p className="text-muted-foreground line-clamp-2 text-[12.5px]">
                          {sched.prompt}
                        </p>
                        <div className="text-muted-foreground flex flex-wrap gap-3 text-[11.5px]">
                          <span>Runs: {sched.run_count}</span>
                          <span>Next: {formatShortTime(sched.next_run_at)}</span>
                          <span>Last: {formatShortTime(sched.last_run_at)}</span>
                        </div>
                      </div>

                      {selectedHostId && (
                        <div className="flex shrink-0 items-center gap-1">
                          <button
                            type="button"
                            title="Run now"
                            onClick={() => {
                              void orbitIpc.runScheduleOnce(selectedHostId, sched.schedule_id);
                              orbitActions.showToast(`Triggered "${sched.name}"`);
                            }}
                            className="border-border hover:bg-accent inline-flex h-[28px] cursor-pointer items-center gap-1 rounded-lg border px-2 text-[11.5px]"
                          >
                            <Zap className="h-[12px] w-[12px]" />
                            <span>Run</span>
                          </button>
                          <button
                            type="button"
                            title={isPaused ? "Resume" : "Pause"}
                            onClick={() =>
                              void (isPaused
                                ? orbitIpc.resumeSchedule(selectedHostId, sched.schedule_id)
                                : orbitIpc.pauseSchedule(selectedHostId, sched.schedule_id))
                            }
                            className="border-border hover:bg-accent inline-flex h-[28px] w-[28px] cursor-pointer items-center justify-center rounded-lg border"
                          >
                            {isPaused ? (
                              <Play className="h-[12.5px] w-[12.5px]" />
                            ) : (
                              <Pause className="h-[12.5px] w-[12.5px]" />
                            )}
                          </button>
                          <button
                            type="button"
                            title="Run logs"
                            onClick={() => void handleToggleRuns(sched)}
                            className="border-border hover:bg-accent inline-flex h-[28px] w-[28px] cursor-pointer items-center justify-center rounded-lg border"
                          >
                            <History className="h-[12.5px] w-[12.5px]" />
                          </button>
                          <button
                            type="button"
                            title="Delete schedule"
                            onClick={() =>
                              void orbitIpc.deleteSchedule(selectedHostId, sched.schedule_id)
                            }
                            className="border-border hover:bg-accent inline-flex h-[28px] w-[28px] cursor-pointer items-center justify-center rounded-lg border text-rose-500"
                          >
                            <Trash2 className="h-[12.5px] w-[12.5px]" />
                          </button>
                        </div>
                      )}
                    </div>

                    {isExpanded && (
                      <div className="border-border bg-muted/35 mt-3 space-y-1.5 rounded-xl border p-2.5 text-[12px]">
                        <div className="text-muted-foreground font-medium text-[11px] uppercase">
                          Recent Runs
                        </div>
                        {runs.length === 0 ? (
                          <div className="text-muted-foreground">No executions recorded yet.</div>
                        ) : (
                          runs.map((r) => (
                            <div
                              key={r.run_id}
                              className="flex items-center justify-between gap-2 py-0.5"
                            >
                              <div className="min-w-0 truncate">
                                <span className="font-medium">{r.status}</span> ·{" "}
                                <span className="text-muted-foreground">
                                  {formatShortTime(r.started_at)}
                                </span>
                                {(r.summary || r.error) && (
                                  <span className="text-muted-foreground">
                                    {" "}
                                    — {(r.summary || r.error || "").slice(0, 120)}
                                  </span>
                                )}
                              </div>
                              {r.agent_id && selectedHostId && (
                                <button
                                  type="button"
                                  onClick={() =>
                                    orbitActions.selectAgent(selectedHostId, r.agent_id!)
                                  }
                                  className="text-primary shrink-0 cursor-pointer text-[11.5px] hover:underline"
                                >
                                  Open agent
                                </button>
                              )}
                            </div>
                          ))
                        )}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </div>

        {/* Live Agent Heartbeats */}
        {heartbeats.length > 0 && (
          <div className="space-y-2.5">
            <div className="text-muted-foreground px-0.5 text-[12.5px] font-medium">
              Active Agent Heartbeats ({heartbeats.length})
            </div>
            <div className="space-y-2">
              {heartbeats.map((hb) => {
                const targetAgentId =
                  hb.target.type === "existing_agent" ? hb.target.agent_id : null;
                const targetAgent = targetAgentId
                  ? hostDetail?.agents.find((a) => a.agent_id === targetAgentId)
                  : null;
                return (
                  <div
                    key={hb.schedule_id}
                    className="border-border bg-elevated/55 flex items-center justify-between gap-3 rounded-2xl border px-4 py-3"
                  >
                    <div className="flex min-w-0 items-center gap-2.5">
                      <HeartPulse className="h-[15px] w-[15px] shrink-0 text-rose-500" />
                      <div className="min-w-0">
                        <div className="flex items-center gap-2 text-[13px] font-medium">
                          <span className="truncate">{hb.name}</span>
                          <code className="bg-muted text-muted-foreground rounded px-1.5 py-0.5 text-[11px]">
                            {hb.cron_expr}
                          </code>
                        </div>
                        <div className="text-muted-foreground truncate text-[12px]">
                          Target: {targetAgent?.title ?? targetAgentId ?? "—"} · Runs: {hb.run_count}{" "}
                          · Next: {formatShortTime(hb.next_run_at)}
                        </div>
                      </div>
                    </div>

                    {selectedHostId && (
                      <div className="flex shrink-0 items-center gap-1.5">
                        {targetAgentId && (
                          <button
                            type="button"
                            onClick={() => orbitActions.selectAgent(selectedHostId, targetAgentId)}
                            className="border-border hover:bg-accent cursor-pointer rounded-lg border px-2 py-1 text-[11.5px]"
                          >
                            Open
                          </button>
                        )}
                        <button
                          type="button"
                          onClick={() =>
                            void orbitIpc.deleteSchedule(selectedHostId, hb.schedule_id)
                          }
                          className="border-border hover:bg-accent cursor-pointer rounded-lg border p-1.5 text-rose-500"
                        >
                          <Trash2 className="h-[12.5px] w-[12.5px]" />
                        </button>
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          </div>
        )}

        {/* Starter Workflow Templates */}
        <div className="space-y-2.5">
          <div className="text-muted-foreground px-0.5 text-[12.5px] font-medium">
            Starter Workflows
          </div>

          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            {TEMPLATES.map((tpl) => {
              const Icon = tpl.icon;
              return (
                <button
                  key={tpl.id}
                  type="button"
                  onClick={() =>
                    orbitActions.startNewThreadDraft(
                      selectedHostId ?? undefined,
                      selectedCwd || undefined,
                      tpl.prompt,
                      tpl.slash ?? null,
                    )
                  }
                  className="border-border bg-elevated/55 hover:bg-elevated orbit-press group flex cursor-pointer flex-col justify-between gap-4 rounded-2xl border p-4.5 text-left hover:border-black/[0.12] dark:hover:border-white/[0.14]"
                >
                  <div className="space-y-2">
                    <div className="flex items-center justify-between gap-2">
                      <span className="bg-muted text-foreground/75 inline-flex h-[30px] w-[30px] items-center justify-center rounded-lg">
                        <Icon className="h-[15.5px] w-[15.5px] stroke-[1.75]" />
                      </span>
                      <ArrowUpRight className="text-muted-foreground group-hover:text-foreground h-[15px] w-[15px] opacity-0 transition-opacity group-hover:opacity-100" />
                    </div>
                    <div className="text-foreground text-[14px] font-medium">{tpl.title}</div>
                    <div className="text-muted-foreground text-[13px] leading-relaxed">
                      {tpl.description}
                    </div>
                  </div>

                  <div className="text-muted-foreground text-[12px] font-medium">
                    {tpl.cadenceLabel}
                  </div>
                </button>
              );
            })}
          </div>
        </div>
      </div>
    </ScrollArea>
  );
}
