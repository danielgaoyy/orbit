import type { MessageKey } from "./i18n/index.ts";
import { formatEffortLabel } from "./model-metadata.ts";
import type {
  AgentCardView,
  BriefingMissionState,
  BriefingWorkerState,
  HostDetailView,
  ProviderKind,
  ReasoningEffort,
} from "./orbit-ipc.ts";
import {
  type ResolvedSubagentStatus,
  formatSubagentRuntimeDisplay,
  isSubagentActiveStatus,
} from "./subagents.ts";

export type AgentListItemStatus = ResolvedSubagentStatus | "idle";

export type AgentListItemTranslateFn = (key: MessageKey) => string;

export function isAgentListItemActiveStatus(status: AgentListItemStatus): boolean {
  return status !== "idle" && isSubagentActiveStatus(status);
}

/**
 * Canonical localized status label shared by `AgentListItem`, `SubagentCrewList`,
 * `PersonalAgentBriefingRail`, and `WorkspaceBoardTab`.
 */
export function formatAgentListItemStatusLabel(
  status: AgentListItemStatus,
  t: AgentListItemTranslateFn,
): string {
  switch (status) {
    case "awaiting_permission":
      return t("overview.subagentApprovalRequired");
    case "running":
      return t("overview.subagentWorking");
    case "failed":
      return t("overview.subagentFailed");
    case "canceled":
      return t("overview.subagentStopped");
    case "completed":
      return t("overview.subagentFinished");
    case "idle":
      return t("overview.subagentIdle");
  }
}

function isAgentStatusCanceled(
  agent: (Pick<AgentCardView, "status_label"> & { status?: { state: string } }) | null | undefined,
): boolean {
  if (!agent) return false;
  const state = agent.status?.state;
  const lowerLabel = agent.status_label?.toLowerCase() ?? "";
  return state === "closed" || /cancel|interrupt|stopped|closed/.test(lowerLabel);
}

function isAgentStatusFailed(
  agent: (Pick<AgentCardView, "status_label"> & { status?: { state: string } }) | null | undefined,
): boolean {
  if (!agent) return false;
  const state = agent.status?.state;
  const lowerLabel = agent.status_label?.toLowerCase() ?? "";
  return state === "error" || /error|fail/.test(lowerLabel);
}

function isAgentForegroundRunningLocal(
  agent: Pick<AgentCardView, "is_running" | "has_background_tasks" | "status_label">,
): boolean {
  if (!agent.is_running) return false;
  if (typeof agent.has_background_tasks === "boolean") {
    return !agent.has_background_tasks;
  }
  const lbl = agent.status_label.trim();
  return !(
    /^waiting\b/i.test(lbl) ||
    /^running\s*\(.*background task/i.test(lbl) ||
    /^background task$/i.test(lbl)
  );
}

/**
 * Maps a `PersonalAgentBriefingView` worker/mission state into the unified `AgentListItemStatus`.
 * When the live `AgentCardView` is available, preserves `"canceled"` for closed/stopped workers.
 */
export function mapBriefingWorkerStateToStatus(
  state: BriefingWorkerState | BriefingMissionState,
  agent?: (Pick<AgentCardView, "status_label"> & { status?: { state: string } }) | null,
): AgentListItemStatus {
  switch (state) {
    case "awaiting":
      return "awaiting_permission";
    case "running":
      return "running";
    case "error":
      return "failed";
    case "done":
      return isAgentStatusCanceled(agent) ? "canceled" : "completed";
    case "idle":
    default:
      return "idle";
  }
}

/**
 * Maps a general `AgentCardView` (e.g., workspace peer threads) into `AgentListItemStatus`.
 * Resting peers stay `"idle"` rather than terminal `"completed"`.
 */
export function mapAgentCardToItemStatus(
  agent: AgentCardView & { status?: { state: string } },
): AgentListItemStatus {
  const state = agent.status?.state;
  if (agent.is_awaiting_permission || state === "awaiting_permission") {
    return "awaiting_permission";
  }
  if (
    isAgentForegroundRunningLocal(agent) ||
    state === "initializing" ||
    state === "running" ||
    (state === "autonomous_wakeup" && !agent.has_background_tasks)
  ) {
    return "running";
  }
  if (isAgentStatusFailed(agent)) {
    return "failed";
  }
  if (isAgentStatusCanceled(agent)) {
    return "canceled";
  }
  return "idle";
}

export interface ResolvedAgentItemRuntime {
  readonly provider: ProviderKind | null;
  readonly agentLabel: string | null;
  readonly modelLabel: string | null;
  readonly rawModel: string | null;
  readonly effort: ReasoningEffort | null;
  readonly effortLabel: string | null;
}

/**
 * Resolves compact provider / model / effort display labels for any agent card using
 * the host's live `provider_catalogs` and `acp_agents` (identical to `resolveSessionSubagents`).
 */
export function resolveAgentItemRuntime(params: {
  provider?: ProviderKind | null;
  agent?: Pick<AgentCardView, "provider" | "acp_agent_id" | "model" | "effort"> | null;
  hostDetail?: Pick<HostDetailView, "provider_catalogs" | "acp_agents"> | null;
  t?: AgentListItemTranslateFn;
}): ResolvedAgentItemRuntime {
  const provider = params.agent?.provider ?? params.provider ?? null;
  const display = formatSubagentRuntimeDisplay({
    provider,
    acpAgentId: params.agent?.acp_agent_id ?? null,
    rawModel: params.agent?.model ?? null,
    effort: params.agent?.effort ?? null,
    providerCatalogs: params.hostDetail?.provider_catalogs,
    acpAgents: params.hostDetail?.acp_agents,
  });

  return {
    provider,
    agentLabel: display.agentLabel,
    modelLabel: display.modelLabel,
    rawModel: display.rawModel,
    effort: display.effort,
    effortLabel: display.effort ? formatEffortLabel(display.effort, params.t) : null,
  };
}

const GENERIC_STATUS_ECHO_RE =
  /^(?:running|working|idle|ready|completed|finished|done|stopped|canceled|cancelled|closed|error|failed|awaiting(?:\s+(?:approval|permission))?|waiting|运行中|空闲|已完成|已停止|已失败|等待审批)[.…!]*$/i;

/**
 * Suppresses redundant status echoes (`"Running"`, `"Idle"`, etc.) or duplicate titles
 * from `step_preview` / `task_summary` so 2-line agent rows stay visually calm.
 */
export function filterDistinctWorkerStepPreview(
  preview: string | null | undefined,
  title: string,
): string | null {
  const trimmed = preview?.trim();
  if (!trimmed) return null;
  if (trimmed.toLowerCase() === title.trim().toLowerCase()) return null;
  if (GENERIC_STATUS_ECHO_RE.test(trimmed)) return null;
  return trimmed;
}

/**
 * Builds a consistent multi-line hover tooltip matching `SubagentCrewList`:
 * Title / lineage on line 1, `Agent · Model Effort · Status · Host` on line 2,
 * and optional prompt / step detail + click hint below.
 */
export function buildAgentListItemTooltip(params: {
  title: string;
  lineage?: string | null;
  agentLabel?: string | null;
  rawModel?: string | null;
  modelLabel?: string | null;
  effortLabel?: string | null;
  statusLabel: string;
  hostName?: string | null;
  worktreeBranch?: string | null;
  detail?: string | null;
  hint?: string | null;
}): string {
  const baseModel = params.rawModel || params.modelLabel || null;
  const modelWithEffort = baseModel
    ? params.effortLabel
      ? `${baseModel} ${params.effortLabel}`
      : baseModel
    : params.effortLabel || null;
  const metaLine = [
    params.agentLabel,
    modelWithEffort,
    params.statusLabel,
    params.hostName,
    params.worktreeBranch?.trim() || null,
  ]
    .filter(Boolean)
    .join(" · ");
  const detailText =
    params.detail && params.detail.trim() !== params.title.trim() ? params.detail.trim() : null;
  return [params.lineage || params.title, metaLine, detailText, params.hint]
    .filter(Boolean)
    .join("\n");
}
