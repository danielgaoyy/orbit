import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowUpRight,
  Check,
  CheckCircle2,
  GitBranch,
  Layers,
  Play,
  Plus,
  RefreshCw,
  Square,
  Trash2,
  Trophy,
  Workflow,
  XCircle,
} from "lucide-react";
import {
  PROVIDER_LIST,
  formatProviderLabel,
  type BestOfNCandidateSpec,
  type DagStepSpec,
  type ProviderKind,
  type SessionMode,
  type WorkflowKind,
  type WorkflowRunRecord,
  type WorkflowStepRecord,
  type WorkflowStepStatus,
} from "@/lib/orbit-ipc";
import {
  SettingSelect,
  ToggleSwitch,
  type SettingSelectOption,
} from "@/components/settings/settings-primitives";
import { Button } from "@/components/ui/button";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/dialog-tabs";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { SimpleTooltip } from "@/components/ui/overlays";
import { Textarea } from "@/components/ui/textarea";
import { orbitActions } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

type LauncherMode = "best_of_n" | "batch_csv" | "dag";
type SimpleSessionMode = "auto" | "accept_edits" | "full_access" | "plan" | "default";

const COLLAPSED_STEP_LIMIT = 12;

interface ProjectChoice {
  path: string;
  name: string;
  branch: string | null;
}

interface CandidateDraft {
  id: string;
  label: string;
  provider: ProviderKind;
  model: string;
  effort: string;
}

interface DagStepDraft {
  id: string;
  stepId: string;
  name: string;
  dependsOn: string;
  promptTemplate: string;
}

const PROVIDER_OPTIONS: SettingSelectOption<ProviderKind>[] = PROVIDER_LIST.map((p) => ({
  value: p.id,
  label: p.label,
}));

const MODE_OPTIONS: SettingSelectOption<SimpleSessionMode>[] = [
  { value: "accept_edits", label: "Accept Edits" },
  { value: "auto", label: "Auto" },
  { value: "full_access", label: "Full Access" },
  { value: "plan", label: "Plan" },
  { value: "default", label: "Ask" },
];

const EFFORT_OPTIONS: SettingSelectOption<string>[] = [
  { value: "", label: "Default" },
  { value: "low", label: "Low" },
  { value: "medium", label: "Medium" },
  { value: "high", label: "High" },
  { value: "xhigh", label: "Extra High" },
];

function kindLabel(kind: WorkflowKind): string {
  switch (kind) {
    case "best_of_n":
      return "Arena";
    case "batch_csv":
      return "Batch CSV";
    case "dag":
      return "DAG";
  }
}

function stepStatusDot(status: WorkflowStepStatus): string {
  switch (status) {
    case "running":
      return "bg-emerald-500 animate-pulse";
    case "verifying":
      return "bg-sky-500 animate-pulse";
    case "succeeded":
      return "bg-emerald-500";
    case "failed":
      return "bg-rose-500";
    case "canceled":
      return "bg-zinc-400";
    case "skipped":
      return "bg-amber-400/75";
    case "pending":
      return "bg-muted-foreground/35";
  }
}

function stepStatusLabel(status: WorkflowStepStatus): string {
  switch (status) {
    case "pending":
      return "Pending";
    case "running":
      return "Running";
    case "verifying":
      return "Verifying";
    case "succeeded":
      return "Done";
    case "failed":
      return "Failed";
    case "canceled":
      return "Canceled";
    case "skipped":
      return "Skipped";
  }
}

function shortCwd(cwd: string): string {
  if (!cwd) return "~";
  const clean = cwd.replace(/\/+$/, "");
  const idx = clean.lastIndexOf("/");
  return idx >= 0 ? clean.slice(idx + 1) || "/" : clean;
}

export interface WorkflowsPanelProps {
  hostId: string | null;
  defaultProvider: ProviderKind;
  defaultCwd: string;
  projectChoices: ProjectChoice[];
  runs: WorkflowRunRecord[];
}

export const WorkflowsPanel = memo(function WorkflowsPanel({
  hostId,
  defaultProvider,
  defaultCwd,
  projectChoices,
  runs,
}: WorkflowsPanelProps) {
  const [launcherMode, setLauncherMode] = useState<LauncherMode>("best_of_n");
  const [cwd, setCwd] = useState(defaultCwd || projectChoices[0]?.path || "");
  const [runName, setRunName] = useState("");
  const [mode, setMode] = useState<SimpleSessionMode>("accept_edits");
  const [launching, setLaunching] = useState(false);

  // Best-of-N Arena state
  const [arenaPrompt, setArenaPrompt] = useState("");
  const [verifyCommand, setVerifyCommand] = useState("");
  const [candidates, setCandidates] = useState<CandidateDraft[]>(() => [
    { id: "c1", label: "Claude", provider: "claude_code", model: "", effort: "" },
    { id: "c2", label: "Codex", provider: "codex", model: "", effort: "" },
  ]);

  // Batch CSV state
  const [batchProvider, setBatchProvider] = useState<ProviderKind>(defaultProvider);
  const [batchCsv, setBatchCsv] = useState("target,task\nsrc/lib.rs,Audit error paths");
  const [batchTemplate, setBatchTemplate] = useState("Inspect {{target}} and {{task}}.");
  const [batchConcurrency, setBatchConcurrency] = useState("3");
  const [batchWorktree, setBatchWorktree] = useState(false);

  // DAG Pipeline state
  const [dagProvider, setDagProvider] = useState<ProviderKind>(defaultProvider);
  const [dagConcurrency, setDagConcurrency] = useState("3");
  const [dagSteps, setDagSteps] = useState<DagStepDraft[]>(() => [
    {
      id: "d1",
      stepId: "analyze",
      name: "Analyze",
      dependsOn: "",
      promptTemplate: "Analyze the target area and list concrete changes.",
    },
    {
      id: "d2",
      stepId: "implement",
      name: "Implement",
      dependsOn: "analyze",
      promptTemplate: "Implement the changes from analysis:\n{{step.analyze.output}}",
    },
  ]);

  const [applyingKey, setApplyingKey] = useState<string | null>(null);
  const [cancelingId, setCancelingId] = useState<string | null>(null);
  const [expandedRunIds, setExpandedRunIds] = useState<Record<string, boolean>>({});

  // Multi-host async guards so switching hosts mid-flight never clobbers the new host's UI state.
  const hostIdRef = useRef<string | null>(hostId);
  const hostEpochRef = useRef(0);
  useEffect(() => {
    if (hostIdRef.current !== hostId) {
      hostIdRef.current = hostId;
      hostEpochRef.current += 1;
      setLaunching(false);
      setApplyingKey(null);
      setCancelingId(null);
      setExpandedRunIds({});
    }
  }, [hostId]);

  // Keep `cwd` strictly scoped to the currently active host's `projectChoices`.
  useEffect(() => {
    const allowed = new Set(projectChoices.map((p) => p.path));
    if (cwd && allowed.has(cwd)) return;
    if (defaultCwd && allowed.has(defaultCwd)) {
      setCwd(defaultCwd);
      return;
    }
    setCwd(projectChoices[0]?.path ?? "");
  }, [hostId, cwd, defaultCwd, projectChoices]);

  const projectOptions = useMemo<SettingSelectOption<string>[]>(() => {
    return projectChoices.map((p) => ({
      value: p.path,
      label: p.name,
      description: p.branch ?? undefined,
    }));
  }, [projectChoices]);

  const addCandidate = useCallback(() => {
    setCandidates((prev) => {
      if (prev.length >= 5) return prev;
      const nextNum = prev.length + 1;
      const fallbackProv: ProviderKind =
        nextNum === 3 ? "antigravity" : nextNum === 4 ? "gemini_cli" : "claude_code";
      return [
        ...prev,
        {
          id: `c_${Date.now()}_${nextNum}`,
          label: `Candidate ${nextNum}`,
          provider: fallbackProv,
          model: "",
          effort: "",
        },
      ];
    });
  }, []);

  const removeCandidate = useCallback((id: string) => {
    setCandidates((prev) => (prev.length <= 2 ? prev : prev.filter((c) => c.id !== id)));
  }, []);

  const addDagStep = useCallback(() => {
    setDagSteps((prev) => {
      if (prev.length >= 64) return prev;
      const idx = prev.length + 1;
      const prevId = prev[prev.length - 1]?.stepId ?? "";
      return [
        ...prev,
        {
          id: `d_${Date.now()}_${idx}`,
          stepId: `step_${idx}`,
          name: `Step ${idx}`,
          dependsOn: prevId,
          promptTemplate: prevId ? `Continue from {{step.${prevId}.output}}` : "",
        },
      ];
    });
  }, []);

  const removeDagStep = useCallback((id: string) => {
    setDagSteps((prev) => (prev.length <= 1 ? prev : prev.filter((s) => s.id !== id)));
  }, []);

  const handleLaunch = useCallback(async () => {
    if (!hostId || launching) return;
    const targetCwd = cwd.trim();
    if (!targetCwd) {
      orbitActions.showToast("Select a workspace first");
      return;
    }

    const activeEpoch = hostEpochRef.current;
    const targetHostId = hostId;
    setLaunching(true);
    try {
      if (launcherMode === "best_of_n") {
        const prompt = arenaPrompt.trim();
        if (!prompt) {
          orbitActions.showToast("Enter a task prompt for Arena candidates");
          return;
        }
        const specs: BestOfNCandidateSpec[] = candidates.map((c) => ({
          label: c.label.trim() || formatProviderLabel(c.provider),
          provider: c.provider,
          model: c.model.trim() || null,
          effort: c.effort.trim() || null,
        }));
        const created = await orbitActions.startBestOfN({
          hostId: targetHostId,
          name: runName.trim(),
          cwd: targetCwd,
          prompt,
          candidates: specs,
          mode: mode as SessionMode,
          verifyCommand: verifyCommand.trim() || null,
        });
        if (created && hostEpochRef.current === activeEpoch) {
          setRunName("");
          setArenaPrompt("");
        }
      } else if (launcherMode === "batch_csv") {
        if (!batchCsv.trim() || !batchTemplate.trim()) {
          orbitActions.showToast("Provide CSV rows and a prompt template");
          return;
        }
        const conc = Math.min(8, Math.max(1, Math.floor(Number(batchConcurrency) || 3)));
        const created = await orbitActions.startBatchFanout({
          hostId: targetHostId,
          name: runName.trim(),
          cwd: targetCwd,
          csvContent: batchCsv,
          promptTemplate: batchTemplate,
          provider: batchProvider,
          mode: mode as SessionMode,
          maxConcurrency: conc,
          isolatedWorktree: batchWorktree,
        });
        if (created && hostEpochRef.current === activeEpoch) {
          setRunName("");
        }
      } else {
        const steps: DagStepSpec[] = dagSteps.map((s) => ({
          step_id: s.stepId.trim(),
          name: s.name.trim() || s.stepId.trim(),
          depends_on: s.dependsOn
            .split(",")
            .map((d) => d.trim())
            .filter(Boolean),
          prompt_template: s.promptTemplate,
          provider: null,
          model: null,
          isolated_worktree: false,
          retry_count: 0,
          timeout_secs: 1800,
        }));
        const conc = Math.min(8, Math.max(1, Math.floor(Number(dagConcurrency) || 3)));
        const created = await orbitActions.startDagWorkflow({
          hostId: targetHostId,
          name: runName.trim(),
          cwd: targetCwd,
          steps,
          defaultProvider: dagProvider,
          defaultMode: mode as SessionMode,
          maxConcurrency: conc,
        });
        if (created && hostEpochRef.current === activeEpoch) {
          setRunName("");
        }
      }
    } finally {
      if (hostEpochRef.current === activeEpoch) {
        setLaunching(false);
      }
    }
  }, [
    hostId,
    launching,
    cwd,
    launcherMode,
    arenaPrompt,
    candidates,
    runName,
    mode,
    verifyCommand,
    batchCsv,
    batchTemplate,
    batchConcurrency,
    batchProvider,
    batchWorktree,
    dagSteps,
    dagConcurrency,
    dagProvider,
  ]);

  const handleCancelRun = useCallback((targetHostId: string, runId: string) => {
    const activeEpoch = hostEpochRef.current;
    setCancelingId(runId);
    void orbitActions.cancelWorkflowRun(targetHostId, runId).finally(() => {
      if (hostEpochRef.current === activeEpoch) {
        setCancelingId((cur) => (cur === runId ? null : cur));
      }
    });
  }, []);

  const handleApplyWinner = useCallback(
    (targetHostId: string, runId: string, stepId: string) => {
      const activeEpoch = hostEpochRef.current;
      const key = `${runId}:${stepId}`;
      setApplyingKey(key);
      void orbitActions.selectBestOfNWinner(targetHostId, runId, stepId, true).finally(() => {
        if (hostEpochRef.current === activeEpoch) {
          setApplyingKey((cur) => (cur === key ? null : cur));
        }
      });
    },
    [],
  );

  return (
    <div className="space-y-6">
      {/* Restrained Segmented Launcher */}
      <div className="border-border/80 bg-elevated/35 space-y-4 rounded-xl border p-4">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <Tabs
            value={launcherMode}
            onValueChange={(v) => setLauncherMode(v as LauncherMode)}
          >
            <TabsList>
              {(
                [
                  { id: "best_of_n", label: "Best-of-N Arena", icon: Trophy },
                  { id: "batch_csv", label: "Batch CSV", icon: Layers },
                  { id: "dag", label: "DAG Pipeline", icon: Workflow },
                ] as const
              ).map((tab) => {
                const Icon = tab.icon;
                return (
                  <TabsTrigger key={tab.id} value={tab.id} className="gap-1.5 text-[12px]">
                    <Icon className="size-3.5 stroke-[1.75]" />
                    <span>{tab.label}</span>
                  </TabsTrigger>
                );
              })}
            </TabsList>
          </Tabs>

          <Button
            type="button"
            size="sm"
            disabled={!hostId || launching || !cwd}
            onClick={() => void handleLaunch()}
          >
            <Play className="size-3 fill-current" />
            <span>{launching ? "Starting…" : "Run"}</span>
          </Button>
        </div>

        {/* Shared compact metadata row */}
        <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-3">
          <Field>
            <FieldLabel className="text-[11.5px]">Workspace</FieldLabel>
            <SettingSelect
              value={cwd}
              onChange={setCwd}
              options={projectOptions}
              placeholder="Select project"
            />
          </Field>
          <Field>
            <FieldLabel htmlFor="workflow-run-name" className="text-[11.5px]">
              Run Name
            </FieldLabel>
            <Input
              id="workflow-run-name"
              value={runName}
              onChange={(e) => setRunName(e.target.value)}
              placeholder="Optional title"
            />
          </Field>
          <Field>
            <FieldLabel className="text-[11.5px]">Permissions</FieldLabel>
            <SettingSelect value={mode} onChange={setMode} options={MODE_OPTIONS} />
          </Field>
        </div>

        {/* Mode-specific inputs */}
        {launcherMode === "best_of_n" && (
          <FieldGroup className="gap-3">
            <Field>
              <FieldLabel htmlFor="workflow-arena-prompt" className="text-[11.5px]">
                Shared Prompt
              </FieldLabel>
              <Textarea
                id="workflow-arena-prompt"
                rows={3}
                value={arenaPrompt}
                onChange={(e) => setArenaPrompt(e.target.value)}
                placeholder="Task solved in parallel across isolated candidate worktrees…"
                className="resize-none leading-relaxed"
              />
            </Field>

            <div className="space-y-2">
              <div className="flex items-center justify-between">
                <FieldLabel className="text-[11.5px]">
                  Candidates ({candidates.length}/5)
                </FieldLabel>
                {candidates.length < 5 && (
                  <Button type="button" variant="ghost" size="xs" onClick={addCandidate}>
                    <Plus className="size-3" />
                    <span>Add candidate</span>
                  </Button>
                )}
              </div>
              <div className="space-y-1.5">
                {candidates.map((cand, idx) => (
                  <div
                    key={cand.id}
                    className="grid grid-cols-[1fr_140px_1fr_104px_26px] items-center gap-1.5"
                  >
                    <Input
                      value={cand.label}
                      onChange={(e) =>
                        setCandidates((prev) =>
                          prev.map((c) => (c.id === cand.id ? { ...c, label: e.target.value } : c)),
                        )
                      }
                      placeholder={`Candidate ${idx + 1}`}
                      className="h-[30px] px-2 text-[12px]"
                    />
                    <SettingSelect
                      value={cand.provider}
                      onChange={(val) =>
                        setCandidates((prev) =>
                          prev.map((c) => (c.id === cand.id ? { ...c, provider: val } : c)),
                        )
                      }
                      options={PROVIDER_OPTIONS}
                    />
                    <Input
                      value={cand.model}
                      onChange={(e) =>
                        setCandidates((prev) =>
                          prev.map((c) => (c.id === cand.id ? { ...c, model: e.target.value } : c)),
                        )
                      }
                      placeholder="Model (default)"
                      className="h-[30px] px-2 font-mono text-[11.5px]"
                    />
                    <SettingSelect
                      value={cand.effort}
                      onChange={(val) =>
                        setCandidates((prev) =>
                          prev.map((c) => (c.id === cand.id ? { ...c, effort: val } : c)),
                        )
                      }
                      options={EFFORT_OPTIONS}
                    />
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      disabled={candidates.length <= 2}
                      onClick={() => removeCandidate(cand.id)}
                      aria-label="Remove candidate"
                      className="text-muted-foreground size-[26px] hover:text-rose-500"
                    >
                      <Trash2 className="size-3.5" />
                    </Button>
                  </div>
                ))}
              </div>
            </div>

            <Field>
              <FieldLabel htmlFor="workflow-verify-cmd" className="text-[11.5px]">
                Verify Command <span className="text-muted-foreground/60 font-normal">(optional)</span>
              </FieldLabel>
              <Input
                id="workflow-verify-cmd"
                value={verifyCommand}
                onChange={(e) => setVerifyCommand(e.target.value)}
                placeholder="e.g. cargo test --quiet"
                className="h-[30px] font-mono text-[12px]"
              />
            </Field>
          </FieldGroup>
        )}

        {launcherMode === "batch_csv" && (
          <FieldGroup className="gap-3">
            <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-3">
              <Field>
                <FieldLabel className="text-[11.5px]">Provider</FieldLabel>
                <SettingSelect
                  value={batchProvider}
                  onChange={setBatchProvider}
                  options={PROVIDER_OPTIONS}
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="workflow-batch-concurrency" className="text-[11.5px]">
                  Concurrency (1–8)
                </FieldLabel>
                <Input
                  id="workflow-batch-concurrency"
                  type="number"
                  min={1}
                  max={8}
                  value={batchConcurrency}
                  onChange={(e) => setBatchConcurrency(e.target.value)}
                  className="font-mono text-[12px]"
                />
              </Field>
              <Field orientation="horizontal" className="items-end pb-1">
                <FieldLabel>Isolated worktree</FieldLabel>
                <ToggleSwitch checked={batchWorktree} onChange={setBatchWorktree} />
              </Field>
            </div>

            <Field>
              <FieldLabel htmlFor="workflow-batch-csv" className="text-[11.5px]">
                CSV Rows (max 200)
              </FieldLabel>
              <Textarea
                id="workflow-batch-csv"
                rows={4}
                value={batchCsv}
                onChange={(e) => setBatchCsv(e.target.value)}
                placeholder="col_a,col_b&#10;val_1,val_2"
                className="resize-y font-mono text-[11.5px] leading-relaxed"
              />
            </Field>

            <Field>
              <FieldLabel htmlFor="workflow-batch-template" className="text-[11.5px]">
                Prompt Template{" "}
                <span className="text-muted-foreground/60 font-mono font-normal">{"{{column}}"}</span>
              </FieldLabel>
              <Textarea
                id="workflow-batch-template"
                rows={2}
                value={batchTemplate}
                onChange={(e) => setBatchTemplate(e.target.value)}
                className="min-h-[60px] resize-none"
              />
            </Field>
          </FieldGroup>
        )}

        {launcherMode === "dag" && (
          <FieldGroup className="gap-3">
            <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
              <Field>
                <FieldLabel className="text-[11.5px]">Default Provider</FieldLabel>
                <SettingSelect
                  value={dagProvider}
                  onChange={setDagProvider}
                  options={PROVIDER_OPTIONS}
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="workflow-dag-concurrency" className="text-[11.5px]">
                  Concurrency (1–8)
                </FieldLabel>
                <Input
                  id="workflow-dag-concurrency"
                  type="number"
                  min={1}
                  max={8}
                  value={dagConcurrency}
                  onChange={(e) => setDagConcurrency(e.target.value)}
                  className="font-mono text-[12px]"
                />
              </Field>
            </div>

            <div className="space-y-2">
              <div className="flex items-center justify-between">
                <FieldLabel className="text-[11.5px]">Steps ({dagSteps.length}/64)</FieldLabel>
                <Button type="button" variant="ghost" size="xs" onClick={addDagStep}>
                  <Plus className="size-3" />
                  <span>Add step</span>
                </Button>
              </div>
              <div className="space-y-2">
                {dagSteps.map((st) => (
                  <div
                    key={st.id}
                    className="border-border/70 bg-canvas/70 space-y-2 rounded-lg border p-2.5"
                  >
                    <div className="grid grid-cols-[110px_1fr_130px_26px] items-center gap-1.5">
                      <Input
                        value={st.stepId}
                        onChange={(e) =>
                          setDagSteps((prev) =>
                            prev.map((x) => (x.id === st.id ? { ...x, stepId: e.target.value } : x)),
                          )
                        }
                        placeholder="step_id"
                        className="h-[28px] px-2 font-mono text-[11.5px]"
                      />
                      <Input
                        value={st.name}
                        onChange={(e) =>
                          setDagSteps((prev) =>
                            prev.map((x) => (x.id === st.id ? { ...x, name: e.target.value } : x)),
                          )
                        }
                        placeholder="Step label"
                        className="h-[28px] px-2 text-[12px]"
                      />
                      <Input
                        value={st.dependsOn}
                        onChange={(e) =>
                          setDagSteps((prev) =>
                            prev.map((x) =>
                              x.id === st.id ? { ...x, dependsOn: e.target.value } : x,
                            ),
                          )
                        }
                        placeholder="depends_on (csv)"
                        className="h-[28px] px-2 font-mono text-[11px]"
                      />
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        disabled={dagSteps.length <= 1}
                        onClick={() => removeDagStep(st.id)}
                        aria-label="Remove step"
                        className="text-muted-foreground size-[26px] hover:text-rose-500"
                      >
                        <Trash2 className="size-3.5" />
                      </Button>
                    </div>
                    <Textarea
                      rows={2}
                      value={st.promptTemplate}
                      onChange={(e) =>
                        setDagSteps((prev) =>
                          prev.map((x) =>
                            x.id === st.id ? { ...x, promptTemplate: e.target.value } : x,
                          ),
                        )
                      }
                      placeholder="Prompt (supports {{step.<id>.output}})"
                      className="min-h-[54px] resize-none px-2.5 py-1.5 text-[12px]"
                    />
                  </div>
                ))}
              </div>
            </div>
          </FieldGroup>
        )}
      </div>

      {/* Live Workflow Runs */}
      <div className="space-y-2.5">
        <div className="flex items-center justify-between px-0.5">
          <div className="text-muted-foreground/80 flex items-center gap-1.5 text-[11.5px] font-medium">
            <span>Workflow Runs</span>
            <span className="text-muted-foreground/60 tabular-nums">{runs.length}</span>
          </div>
          {hostId && (
            <button
              type="button"
              onClick={() => void orbitActions.refreshWorkflowRuns(hostId)}
              className="text-muted-foreground hover:text-foreground inline-flex cursor-pointer items-center gap-1 text-[11.5px]"
            >
              <RefreshCw className="size-3" />
              <span>Refresh</span>
            </button>
          )}
        </div>

        {runs.length === 0 ? (
          <div className="border-border/75 text-muted-foreground rounded-xl border border-dashed px-4 py-8 text-center text-[12.5px]">
            No workflow runs yet. Launch a Best-of-N Arena, Batch CSV, or DAG Pipeline above.
          </div>
        ) : (
          <div className="space-y-3">
            {runs.map((run) => {
              const isActive = run.status === "pending" || run.status === "running";
              const completedCount = run.steps.filter((s) => s.status === "succeeded").length;
              const isExpanded = Boolean(expandedRunIds[run.run_id]);
              const hasHiddenSteps = run.steps.length > COLLAPSED_STEP_LIMIT;
              const visibleSteps =
                hasHiddenSteps && !isExpanded
                  ? run.steps.slice(0, COLLAPSED_STEP_LIMIT)
                  : run.steps;

              return (
                <div
                  key={run.run_id}
                  className="border-border/80 bg-elevated/35 overflow-hidden rounded-xl border"
                >
                  {/* Card Header */}
                  <div className="border-border/70 flex items-center justify-between gap-3 border-b px-4 py-2.5">
                    <div className="flex min-w-0 items-center gap-2">
                      <span
                        className={cn(
                          "size-1.5 shrink-0 rounded-full",
                          isActive
                            ? "bg-emerald-500 animate-pulse"
                            : run.status === "succeeded"
                              ? "bg-emerald-500"
                              : run.status === "partial_failure"
                                ? "bg-amber-500"
                                : run.status === "failed"
                                  ? "bg-rose-500"
                                  : "bg-zinc-400",
                        )}
                      />
                      <span className="bg-muted text-muted-foreground shrink-0 rounded px-1.5 py-0.5 text-[10.5px] font-medium">
                        {kindLabel(run.kind)}
                      </span>
                      <span className="text-foreground truncate text-[13px] font-medium">
                        {run.name}
                      </span>
                      <span className="text-muted-foreground/70 tabular-nums shrink-0 font-mono text-[11px]">
                        {completedCount}/{run.steps.length}
                      </span>
                      <span className="text-muted-foreground/65 hidden truncate font-mono text-[11px] sm:inline">
                        · {shortCwd(run.workspace_cwd)}
                      </span>
                    </div>

                    {isActive && hostId && (
                      <button
                        type="button"
                        disabled={cancelingId === run.run_id}
                        onClick={() => handleCancelRun(hostId, run.run_id)}
                        className="text-muted-foreground hover:bg-accent inline-flex h-[26px] cursor-pointer items-center gap-1 rounded-md px-2 text-[11.5px] font-medium transition-colors hover:text-rose-500 disabled:opacity-40"
                      >
                        <Square className="size-3 stroke-[2]" />
                        <span>Cancel</span>
                      </button>
                    )}
                  </div>

                  {/* Step / Candidate Rows */}
                  <div className="divide-border/60 divide-y">
                    {visibleSteps.map((step: WorkflowStepRecord) => {
                      const hasDiff =
                        step.diff_files > 0 || step.diff_additions > 0 || step.diff_deletions > 0;
                      const canApplyWinner =
                        run.kind === "best_of_n" &&
                        Boolean(step.worktree_cwd) &&
                        step.status === "succeeded" &&
                        !run.winner_step_id;
                      const applyKey = `${run.run_id}:${step.step_id}`;

                      return (
                        <div
                          key={step.step_id}
                          className="flex flex-wrap items-center justify-between gap-2 px-4 py-2 text-[12px] transition-colors hover:bg-black/[0.015] dark:hover:bg-white/[0.02]"
                        >
                          <div className="flex min-w-0 flex-1 items-center gap-2">
                            <span
                              className={cn(
                                "size-1.5 shrink-0 rounded-full",
                                stepStatusDot(step.status),
                              )}
                            />
                            <span className="text-foreground truncate font-medium">
                              {step.name}
                            </span>
                            <span
                              title={`${formatProviderLabel(step.provider)}${step.model ? ` · ${step.model}` : ""}`}
                              className="text-muted-foreground/75 max-w-[45%] shrink-0 truncate text-[11px]"
                            >
                              {formatProviderLabel(step.provider)}
                              {step.model ? ` · ${step.model}` : ""}
                            </span>
                            <span className="bg-muted/80 text-muted-foreground shrink-0 rounded px-1.5 py-0.5 text-[10.5px]">
                              {stepStatusLabel(step.status)}
                            </span>
                            {step.is_winner && (
                              <span className="inline-flex shrink-0 items-center gap-1 rounded bg-emerald-500/15 px-1.5 py-0.5 text-[10.5px] font-medium text-emerald-600 dark:text-emerald-400">
                                <Trophy className="size-2.5" />
                                <span>Winner</span>
                              </span>
                            )}
                            {step.worktree_cwd && (
                              <GitBranch className="text-muted-foreground/55 size-3 shrink-0" />
                            )}
                          </div>

                          <div className="flex shrink-0 items-center gap-2">
                            {hasDiff && (
                              <span className="tabular-nums font-mono text-[11px]">
                                <span className="text-emerald-600 dark:text-emerald-400">
                                  +{step.diff_additions}
                                </span>
                                <span className="text-muted-foreground/50 mx-0.5">/</span>
                                <span className="text-rose-600 dark:text-rose-400">
                                  -{step.diff_deletions}
                                </span>
                              </span>
                            )}

                            {step.verify_passed !== null && step.verify_passed !== undefined && (
                              <SimpleTooltip
                                label={
                                  step.verify_output
                                    ? step.verify_output.slice(-240)
                                    : step.verify_passed
                                      ? "Verification passed"
                                      : "Verification failed"
                                }
                              >
                                <span
                                  className={cn(
                                    "inline-flex items-center gap-0.5 rounded px-1.5 py-0.5 text-[10.5px] font-medium",
                                    step.verify_passed
                                      ? "bg-emerald-500/12 text-emerald-600 dark:text-emerald-400"
                                      : "bg-rose-500/12 text-rose-600 dark:text-rose-400",
                                  )}
                                >
                                  {step.verify_passed ? (
                                    <CheckCircle2 className="size-3" />
                                  ) : (
                                    <XCircle className="size-3" />
                                  )}
                                  <span>{step.verify_passed ? "Pass" : "Fail"}</span>
                                </span>
                              </SimpleTooltip>
                            )}

                            {step.error && (
                              <SimpleTooltip label={step.error}>
                                <span className="max-w-36 truncate text-[11px] text-rose-500">
                                  {step.error}
                                </span>
                              </SimpleTooltip>
                            )}

                            {hostId && step.agent_id && (
                              <button
                                type="button"
                                onClick={() => orbitActions.selectAgent(hostId, step.agent_id!)}
                                className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[24px] cursor-pointer items-center gap-0.5 rounded-md px-1.5 text-[11px] font-medium transition-colors"
                              >
                                <span>Open</span>
                                <ArrowUpRight className="size-3" />
                              </button>
                            )}

                            {hostId && canApplyWinner && (
                              <button
                                type="button"
                                disabled={applyingKey !== null}
                                onClick={() => handleApplyWinner(hostId, run.run_id, step.step_id)}
                                className="bg-foreground text-background hover:opacity-90 inline-flex h-[24px] cursor-pointer items-center gap-1 rounded-md px-2 text-[11px] font-medium transition-opacity disabled:opacity-40"
                              >
                                <Check className="size-3" />
                                <span>
                                  {applyingKey === applyKey ? "Applying…" : "Apply Winner"}
                                </span>
                              </button>
                            )}
                          </div>
                        </div>
                      );
                    })}

                    {hasHiddenSteps && (
                      <div className="px-4 py-2 text-center">
                        <button
                          type="button"
                          onClick={() =>
                            setExpandedRunIds((prev) => ({
                              ...prev,
                              [run.run_id]: !prev[run.run_id],
                            }))
                          }
                          className="text-muted-foreground hover:text-foreground cursor-pointer text-[11.5px] font-medium transition-colors"
                        >
                          {isExpanded
                            ? "Show fewer steps"
                            : `Show all ${run.steps.length} steps`}
                        </button>
                      </div>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
});
