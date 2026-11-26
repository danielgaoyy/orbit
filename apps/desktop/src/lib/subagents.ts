import type {
  AcpAgentAvailability,
  AgentCardView,
  HostCardView,
  HostDetailView,
  ProviderKind,
  ProviderModelCatalog,
  ProviderSubagentDescriptor,
  ReasoningEffort,
  SessionSubagentEntryView,
  SubagentLineageStepView,
  TimelineItemView,
} from "./orbit-ipc.ts";
import {
  formatCatalogModelLabel,
  formatProviderDisplayName,
  prettifyModelSlug,
  stripAgyEffortSuffix,
} from "./model-metadata.ts";

export const MAX_SUBAGENT_LINEAGE_DEPTH = 8;

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

export type ResolvedSubagentStatus =
  | "awaiting_permission"
  | "running"
  | "failed"
  | "canceled"
  | "completed";

export interface ResolvedSubagentItem {
  /** Stable unique key for React lists */
  key: string;
  /** Call ID passed to openSubagentTab */
  callId: string;
  /** A title-inferred dispatch must yield to another launch with an explicit child ID. */
  callIdInferred?: boolean;
  handle?: string | null;
  /** Human-readable role or thread title */
  title: string;
  /** Prompt / subtitle preview for tooltips and SubagentTab fallback */
  promptPreview: string;
  /** Unified lifecycle status */
  status: ResolvedSubagentStatus;
  /** True for Orbit managed child threads (same-host or cross-host); false for provider native read-only */
  isManaged: boolean;
  /** Host ID where this subagent lives (may differ from parent hostId for cross-host delegation) */
  childHostId: string;
  /** Non-null only when childHostId !== parent hostId */
  remoteHostName: string | null;
  /** Managed child agent ID when isManaged === true */
  childAgentId: string | null;
  /** Real provider engine (from managed child card, or inherited from parent for provider-native subagents) */
  childProvider: ProviderKind | null;
  /** Optional ACP agent ID when childProvider === "acp" */
  childAcpAgentId?: string | null;
  /** Real model slug used by this subagent when known (never synthesized as "Default") */
  childModel?: string | null;
  /** Real thinking/reasoning effort when explicitly configured or inferred from model slug */
  childEffort?: ReasoningEffort | null;
  /** Pre-formatted human-readable Agent label (e.g. "Claude Code", "Codex", "Antigravity", or ACP agent name) */
  agentLabel?: string | null;
  /** Pre-formatted compact model label (e.g. "Sonnet 4.6", "GPT 5.4", "Gemini 4 Neon"), or null when unknown/default */
  modelLabel?: string | null;
  /** Optional git worktree branch when this managed subagent runs in an isolated worktree */
  childWorktreeBranch?: string | null;
  /** Provider native subagent ID when read-only native subagent */
  providerSubagentId: string | null;
  /** Managed card creation time, or zero when unavailable. */
  createdAtMs: number;
  /** Start of the currently observed run, including a restart after settlement. */
  activeStartedAtMs?: number | null;
  /** Sort tie-breaker (higher = more recent) */
  sortOrder: number;
  /** Nesting depth relative to the inspected root session (1 = direct child, 2 = grandchild, up to 8). */
  depth?: number;
  /** Direct parent agent ID in multi-level subagent trees. */
  parentAgentId?: string | null;
  /** Direct parent host ID in cross-host multi-level subagent trees. */
  parentHostId?: string | null;
  /** Full breadcrumb lineage from root ancestor to this subagent. */
  lineage?: readonly SubagentLineageStepView[];
}

export interface SubagentRuntimeDisplay {
  agentLabel: string | null;
  modelLabel: string | null;
  rawModel: string | null;
  effort: ReasoningEffort | null;
}

const LEGACY_CLAUDE_SUBTITLE_MODEL_RE = /^(?:claude-[a-z0-9._-]+|opus|sonnet|haiku)$/i;

/**
 * Extracts a real model slug from a provider-native subagent descriptor.
 * Prefers the explicit structured `model` field and falls back cleanly for older daemons
 * where Claude Code placed the model slug inside `subtitle`.
 */
export function inferNativeSubagentModel(
  provider: ProviderKind | null | undefined,
  descriptor: Pick<ProviderSubagentDescriptor, "model" | "subtitle"> | null | undefined,
): string | null {
  if (!descriptor) return null;
  const explicit = descriptor.model?.trim();
  if (explicit) return explicit;
  if (provider === "claude_code") {
    const legacy = descriptor.subtitle?.trim();
    if (legacy && LEGACY_CLAUDE_SUBTITLE_MODEL_RE.test(legacy)) {
      return legacy;
    }
  }
  return null;
}

/**
 * Resolves compact, human-readable Agent and Model labels shared across the Overview subagent list,
 * TopBar mini-dock tooltip, and the Side Panel `SubagentTab` header.
 * Never fabricates `"Default"` when no real model slug is known.
 */
export function formatSubagentRuntimeDisplay(params: {
  provider: ProviderKind | null | undefined;
  acpAgentId?: string | null;
  rawModel?: string | null;
  effort?: ReasoningEffort | null;
  providerCatalogs?: readonly ProviderModelCatalog[];
  acpAgents?: readonly AcpAgentAvailability[];
}): SubagentRuntimeDisplay {
  const { provider, acpAgentId, providerCatalogs, acpAgents } = params;
  const rawModel = params.rawModel?.trim() || null;
  const effort: ReasoningEffort | null =
    params.effort ??
    (provider === "antigravity" && rawModel
      ? stripAgyEffortSuffix(rawModel).inferredEffort
      : null);

  let agentLabel: string | null = null;
  if (provider === "acp") {
    const matchedAcp = acpAgentId
      ? acpAgents?.find((a) => a.spec.id === acpAgentId)?.spec.name?.trim()
      : null;
    agentLabel = matchedAcp || acpAgentId?.trim() || formatProviderDisplayName("acp");
  } else if (provider) {
    agentLabel = formatProviderDisplayName(provider);
  }

  let modelLabel: string | null = null;
  if (rawModel) {
    if (provider === "acp") {
      modelLabel = rawModel;
    } else if (provider) {
      const catalog = providerCatalogs?.find((c) => c.provider === provider);
      const formatted =
        catalog && catalog.models.length > 0
          ? formatCatalogModelLabel(catalog, rawModel)
          : prettifyModelSlug(rawModel, provider);
      const compact =
        provider === "claude_code"
          ? formatted.replace(/^Claude\s+/i, "").trim()
          : formatted.trim();
      modelLabel = compact && compact.toLowerCase() !== "default" ? compact : null;
    } else {
      const formatted = prettifyModelSlug(rawModel).trim();
      modelLabel = formatted && formatted.toLowerCase() !== "default" ? formatted : null;
    }
  }

  return { agentLabel, modelLabel, rawModel, effort };
}

const STATUS_PRIORITY: Record<ResolvedSubagentStatus, number> = {
  awaiting_permission: 0,
  running: 1,
  failed: 2,
  canceled: 3,
  completed: 4,
};

// Rust serializes AgentStatus on AgentCardView. Older desktop snapshots only have the flags/label.
export function mapManagedStatus(
  agent: AgentCardView & { status?: { state: string; source?: string } },
): ResolvedSubagentStatus {
  const state = agent.status?.state;
  if (agent.is_awaiting_permission || state === "awaiting_permission") return "awaiting_permission";
  const isWakeupWaiting =
    state === "autonomous_wakeup" &&
    typeof agent.status?.source === "string" &&
    agent.status.source.trim().startsWith("waiting:");
  const isExplicitNonRunning =
    state === "idle" || state === "closed" || state === "error" || isWakeupWaiting;
  if (
    !isExplicitNonRunning &&
    !agent.has_background_tasks &&
    (state === "initializing" ||
      state === "running" ||
      state === "autonomous_wakeup" ||
      isAgentForegroundRunningLocal(agent))
  ) {
    return "running";
  }
  const lowerLabel = agent.status_label?.toLowerCase() ?? "";
  if (state === "error" || /error|fail/.test(lowerLabel)) return "failed";
  if (state === "closed" || /cancel|interrupt|stopped|closed/.test(lowerLabel)) return "canceled";
  return "completed";
}

function mapProviderSubagentStatus(
  status: ProviderSubagentDescriptor["status"],
): ResolvedSubagentStatus {
  if (status === "running") return "running";
  if (status === "completed") return "completed";
  if (status === "canceled" || status === "stopped") return "canceled";
  return "failed";
}

function mapInsightStatus(status: SessionSubagentEntryView["status"]): ResolvedSubagentStatus {
  if (status === "running") return "running";
  if (status === "failed" || status === "denied") return "failed";
  return "completed";
}

function parseTimestampMs(rfc3339: string | null | undefined): number {
  if (!rfc3339) return 0;
  const parsed = Date.parse(rfc3339);
  return Number.isFinite(parsed) ? parsed : 0;
}

type ManagedRun = Pick<ResolvedSubagentItem, "status" | "activeStartedAtMs">;
type ManagedRuns = Map<string, ManagedRun>;
const runsByHost = new Map<string, {
  latest: ManagedRuns;
  snapshots: WeakMap<readonly AgentCardView[], ManagedRuns>;
}>();

// Track transitions on fleet snapshots so scrolling a dispatch offscreen doesn't reset its clock.
// A replay of a snapshot by another selector must return the same run and start time.
function managedRuns(hostId: string, agents: readonly AgentCardView[]): ManagedRuns {
  let history = runsByHost.get(hostId);
  if (!history) {
    history = { latest: new Map(), snapshots: new WeakMap() };
    runsByHost.set(hostId, history);
  }
  const cached = history.snapshots.get(agents);
  if (cached) return cached;

  const runs: ManagedRuns = new Map();
  for (const agent of agents) {
    const status = mapManagedStatus(agent);
    let activeStartedAtMs = null;
    if (isSubagentActiveStatus(status)) {
      const previous = history.latest.get(agent.agent_id);
      activeStartedAtMs = previous && !isSubagentActiveStatus(previous.status)
        ? Date.now()
        : previous?.activeStartedAtMs ?? (parseTimestampMs(agent.created_at_rfc3339) || null);
    }
    runs.set(agent.agent_id, { status, activeStartedAtMs });
  }
  history.latest = runs;
  history.snapshots.set(agents, runs);
  return runs;
}

function normalizeSubagentName(value: string): string {
  return value.trim().toLowerCase().replace(/^@/, "").replace(/[-_\s]+/g, " ").trim();
}

function buildHostNameMap(
  fleetHosts: readonly HostCardView[],
  hostDetails: Record<string, HostDetailView>,
): Map<string, string> {
  const hostNameById = new Map<string, string>();
  for (const h of fleetHosts) {
    hostNameById.set(h.host_id, h.display_name || h.host_id);
  }
  for (const [hid, detail] of Object.entries(hostDetails)) {
    if (!hostNameById.has(hid)) {
      hostNameById.set(hid, detail.host.display_name || hid);
    }
  }
  return hostNameById;
}

/**
 * Resolves the cycle-safe, depth-bounded (`<= 8`) ancestor breadcrumb chain (`Root -> ... -> Current`)
 * for an agent. Prefers Rust ViewModel `stageAncestorLineage` when present, falling back to live
 * `hostDetails` traversal before stage hydration completes.
 */
export function resolveAgentAncestorLineage(params: {
  hostId: string | null | undefined;
  agentId: string | null | undefined;
  stageAncestorLineage?: readonly SubagentLineageStepView[] | null;
  hostDetails: Record<string, HostDetailView>;
  fleetHosts: readonly HostCardView[];
}): SubagentLineageStepView[] {
  const { hostId, agentId, stageAncestorLineage, hostDetails, fleetHosts } = params;
  if (stageAncestorLineage && stageAncestorLineage.length > 0) {
    return [...stageAncestorLineage];
  }
  if (!hostId || !agentId) return [];

  const hostNameById = buildHostNameMap(fleetHosts, hostDetails);
  const byComposite = new Map<string, { hostId: string; agent: AgentCardView }>();
  const byAgentId = new Map<string, { hostId: string; agent: AgentCardView }>();
  for (const [hid, detail] of Object.entries(hostDetails)) {
    for (const ag of detail.agents) {
      byComposite.set(`${hid}::${ag.agent_id}`, { hostId: hid, agent: ag });
      if (!byAgentId.has(ag.agent_id)) {
        byAgentId.set(ag.agent_id, { hostId: hid, agent: ag });
      }
    }
  }

  const chain: SubagentLineageStepView[] = [];
  const visited = new Set<string>();
  let curHostId = hostId;
  let curAgentId = agentId;

  for (let hop = 0; hop <= MAX_SUBAGENT_LINEAGE_DEPTH; hop += 1) {
    const key = `${curHostId}::${curAgentId}`;
    if (visited.has(key)) break;
    visited.add(key);

    const found = byComposite.get(key) ?? byAgentId.get(curAgentId);
    if (!found) break;

    chain.push({
      host_id: found.hostId,
      host_name: hostNameById.get(found.hostId) ?? found.hostId,
      agent_id: found.agent.agent_id,
      title: found.agent.title || found.agent.agent_id,
    });

    const nextParentId = found.agent.parent_agent_id;
    if (!nextParentId) break;
    const nextParentHost = found.agent.parent_host_id || found.hostId;
    curHostId = nextParentHost;
    curAgentId = nextParentId;
  }

  chain.reverse();
  return chain;
}

/**
 * Resolves a single unified, deduplicated multi-level tree of subagents for a parent conversation,
 * combining:
 * 1. Rust-projected `stage.insights.subagents` (recursive cross-host descendants up to depth 8 + timeline calls)
 * 2. Live Orbit managed descendants across ALL hosts (cycle-safe, max depth 8)
 * 3. Live provider native subagent descriptors from `hostDetails[hostId].provider_subagents`
 */
export function resolveSessionSubagents(params: {
  hostId: string | null;
  agentId: string | null;
  insightsSubagents: readonly SessionSubagentEntryView[];
  hostDetails: Record<string, HostDetailView>;
  fleetHosts: readonly HostCardView[];
}): ResolvedSubagentItem[] {
  const { hostId, agentId, insightsSubagents, hostDetails, fleetHosts } = params;
  if (!hostId || !agentId) return [];

  const hostNameById = buildHostNameMap(fleetHosts, hostDetails);

  // Build global agent lookup and parent -> direct-children adjacency across all hosts.
  type ManagedNode = {
    childHostId: string;
    remoteHostName: string | null;
    agent: AgentCardView;
    run: ManagedRun;
  };
  const allAgentsByComposite = new Map<string, ManagedNode>();
  const allAgentsById = new Map<string, ManagedNode>();
  const childrenByParent = new Map<string, ManagedNode[]>();
  const parentAgent = hostDetails[hostId]?.agents.find((a) => a.agent_id === agentId) ?? null;
  const parentProvider = parentAgent?.provider ?? null;
  const parentAcpAgentId = parentProvider === "acp" ? (parentAgent?.acp_agent_id ?? null) : null;
  const rootNode = {
    host_id: hostId,
    host_name: hostNameById.get(hostId) ?? hostId,
    agent_id: agentId,
    title: parentAgent?.title || agentId,
  } satisfies SubagentLineageStepView;

  const formatRuntimeForHost = (
    targetHostId: string,
    provider: ProviderKind | null,
    acpAgentId: string | null,
    rawModel: string | null,
    effort: ReasoningEffort | null,
  ) => {
    const detail = hostDetails[targetHostId] ?? hostDetails[hostId];
    return formatSubagentRuntimeDisplay({
      provider,
      acpAgentId,
      rawModel,
      effort,
      providerCatalogs: detail?.provider_catalogs,
      acpAgents: detail?.acp_agents,
    });
  };

  for (const [hid, detail] of Object.entries(hostDetails)) {
    const runs = managedRuns(hid, detail.agents);
    for (const ag of detail.agents) {
      const node: ManagedNode = {
        childHostId: hid,
        remoteHostName: hid !== hostId ? (hostNameById.get(hid) ?? hid) : null,
        agent: ag,
        run: runs.get(ag.agent_id)!,
      };
      allAgentsByComposite.set(`${hid}::${ag.agent_id}`, node);
      if (!allAgentsById.has(ag.agent_id)) {
        allAgentsById.set(ag.agent_id, node);
      }
      if (ag.parent_agent_id) {
        const pHost = ag.parent_host_id || hid;
        const pKey = `${pHost}::${ag.parent_agent_id}`;
        const list = childrenByParent.get(pKey);
        if (list) list.push(node);
        else childrenByParent.set(pKey, [node]);
      }
    }
  }

  // 1. Collect cycle-safe recursive descendants (max depth 8) across all hosts + dispatched peers
  const managedById = new Map<
    string,
    ManagedNode & {
      depth: number;
      parentAgentId: string;
      parentHostId: string;
      lineage: SubagentLineageStepView[];
    }
  >();
  const archivedManagedIds = new Set<string>();
  const visitedNodes = new Set<string>([`${hostId}::${agentId}`]);

  const queue: Array<{
    node: ManagedNode;
    depth: number;
    parentAgentId: string;
    parentHostId: string;
    lineage: SubagentLineageStepView[];
  }> = [];

  const enqueueNode = (
    node: ManagedNode,
    depth: number,
    parentAgentId: string,
    parentHostId: string,
    parentLineage: readonly SubagentLineageStepView[],
  ) => {
    if (depth > MAX_SUBAGENT_LINEAGE_DEPTH) return;
    const composite = `${node.childHostId}::${node.agent.agent_id}`;
    if (visitedNodes.has(composite)) return;
    visitedNodes.add(composite);

    const step: SubagentLineageStepView = {
      host_id: node.childHostId,
      host_name: hostNameById.get(node.childHostId) ?? node.childHostId,
      agent_id: node.agent.agent_id,
      title: node.agent.title || node.agent.agent_id,
    };
    const nextLineage = [...parentLineage, step];

    if (node.agent.archived_at_rfc3339 && !isSubagentActiveStatus(node.run.status)) {
      archivedManagedIds.add(node.agent.agent_id);
    } else {
      managedById.set(node.agent.agent_id, {
        ...node,
        depth,
        parentAgentId,
        parentHostId,
        lineage: nextLineage,
      });
    }

    queue.push({
      node,
      depth,
      parentAgentId,
      parentHostId,
      lineage: nextLineage,
    });
  };

  for (const child of childrenByParent.get(`${hostId}::${agentId}`) ?? []) {
    enqueueNode(child, 1, agentId, hostId, [rootNode]);
  }

  // Workspace peers have no parent pointer; their Rust insight entry identifies them.
  for (const sa of insightsSubagents) {
    if (!sa.child_agent_id?.startsWith("ag_")) continue;
    const node =
      (sa.host_id ? allAgentsByComposite.get(`${sa.host_id}::${sa.child_agent_id}`) : undefined) ??
      allAgentsById.get(sa.child_agent_id);
    if (!node) continue;
    const insightDepth = sa.depth && sa.depth > 0 ? Math.min(sa.depth, MAX_SUBAGENT_LINEAGE_DEPTH) : 1;
    const pAgent = sa.parent_agent_id || node.agent.parent_agent_id || agentId;
    const pHost = sa.parent_host_id || node.agent.parent_host_id || hostId;
    const seedLineage =
      sa.lineage && sa.lineage.length > 1
        ? sa.lineage.slice(0, sa.lineage.length - 1)
        : [rootNode];
    enqueueNode(node, insightDepth, pAgent, pHost, seedLineage);
  }

  // BFS/recursive expansion for grandchildren up to MAX_SUBAGENT_LINEAGE_DEPTH
  for (let head = 0; head < queue.length; head += 1) {
    const current = queue[head];
    if (current.depth >= MAX_SUBAGENT_LINEAGE_DEPTH) continue;
    const nestedKey = `${current.node.childHostId}::${current.node.agent.agent_id}`;
    for (const child of childrenByParent.get(nestedKey) ?? []) {
      enqueueNode(
        child,
        current.depth + 1,
        current.node.agent.agent_id,
        current.node.childHostId,
        current.lineage,
      );
    }
  }

  // 2. Index live provider native subagents on the parent host
  const providerList = hostDetails[hostId]?.provider_subagents?.[agentId] ?? [];
  const providerById = new Map<string, ProviderSubagentDescriptor>();
  for (const ps of providerList) {
    providerById.set(ps.subagent_id, ps);
  }

  // Core may emit both a dispatch insight and a synthetic card insight (id === child_agent_id).
  // Reserve real ID-bearing calls first. Match a missing ID only for one launch and one free card.
  const managedInsights = new Map<string, SessionSubagentEntryView>();
  const launchesByTitle = new Map<string, SessionSubagentEntryView | null>();
  for (const sa of insightsSubagents) {
    if (sa.child_agent_id) {
      if (!managedInsights.has(sa.child_agent_id) || sa.id !== sa.child_agent_id) {
        managedInsights.set(sa.child_agent_id, sa);
      }
    } else if (!sa.provider_subagent_id && (sa.status === "running" || sa.status === "completed")) {
      const title = normalizeSubagentName(sa.role);
      if (title && !GENERIC_SUBAGENT_TITLES.has(title)) {
        launchesByTitle.set(title, launchesByTitle.has(title) ? null : sa);
      }
    }
  }
  const cardsByTitle = new Map<string, string | null>();
  for (const [id, { agent, depth }] of managedById) {
    if (depth > 1) continue;
    const insight = managedInsights.get(id);
    if (insight && insight.id !== id) continue;
    const title = normalizeSubagentName(agent.title);
    if (launchesByTitle.get(title)) cardsByTitle.set(title, cardsByTitle.has(title) ? null : id);
  }
  const reconciled = new Set<SessionSubagentEntryView>();
  for (const [title, id] of cardsByTitle) {
    const launch = launchesByTitle.get(title);
    if (id && launch) {
      managedInsights.set(id, launch);
      reconciled.add(launch);
    }
  }
  const consumedProviderIds = new Set<string>();
  const out: ResolvedSubagentItem[] = [];

  // 3. Walk Rust-projected insights first to preserve timeline prompt previews & call IDs
  for (const sa of insightsSubagents) {
    if (reconciled.has(sa) || (sa.child_agent_id &&
        (archivedManagedIds.has(sa.child_agent_id) || managedById.has(sa.child_agent_id)))) {
      continue;
    }

    const insightHostId = sa.host_id || hostId;
    const insightRemoteHost =
      insightHostId !== hostId ? (sa.host_name || hostNameById.get(insightHostId) || insightHostId) : null;
    const insightDepth = sa.depth && sa.depth > 0 ? Math.min(sa.depth, MAX_SUBAGENT_LINEAGE_DEPTH) : 1;

    const providerSub = sa.provider_subagent_id
      ? providerById.get(sa.provider_subagent_id)
      : undefined;
    if (providerSub) {
      consumedProviderIds.add(providerSub.subagent_id);
      const nativeModel = inferNativeSubagentModel(parentProvider, providerSub);
      const runtime = formatRuntimeForHost(
        insightHostId,
        parentProvider,
        parentAcpAgentId,
        nativeModel,
        null,
      );
      // If the spawn tool call itself failed, a stale persisted `running` provider descriptor
      // from an older daemon must not resurrect the failed launch back to `running`.
      const resolvedStatus: ResolvedSubagentStatus =
        sa.status === "failed" && providerSub.status === "running"
          ? "failed"
          : mapProviderSubagentStatus(providerSub.status);
      out.push({
        key: `provider:${providerSub.subagent_id}`,
        callId: sa.id,
        title: providerSub.title?.trim() || sa.role || "Subagent",
        promptPreview: sa.prompt_preview || providerSub.subtitle || "",
        status: resolvedStatus,
        isManaged: false,
        childHostId: insightHostId,
        remoteHostName: insightRemoteHost,
        childAgentId: null,
        childProvider: parentProvider,
        childAcpAgentId: parentAcpAgentId,
        childModel: nativeModel,
        childEffort: runtime.effort,
        agentLabel: runtime.agentLabel,
        modelLabel: runtime.modelLabel,
        childWorktreeBranch: null,
        providerSubagentId: providerSub.subagent_id,
        createdAtMs: 0,
        sortOrder: Math.max(sa.last_seq, parseTimestampMs(providerSub.updated_at)),
        depth: insightDepth,
        parentAgentId: sa.parent_agent_id ?? agentId,
        parentHostId: sa.parent_host_id ?? hostId,
        lineage: sa.lineage && sa.lineage.length > 0 ? sa.lineage : undefined,
      });
      continue;
    }

    const insightRuntime = formatRuntimeForHost(
      insightHostId,
      parentProvider,
      parentAcpAgentId,
      null,
      null,
    );
    out.push({
      key: `insight:${sa.id}`,
      callId: sa.id,
      title: sa.role || "Subagent",
      promptPreview: sa.prompt_preview || "",
      status: sa.child_agent_id?.startsWith("ag_") && sa.status === "completed"
        ? "running" : mapInsightStatus(sa.status),
      isManaged: false,
      childHostId: insightHostId,
      remoteHostName: insightRemoteHost,
      childAgentId: null,
      childProvider: parentProvider,
      childAcpAgentId: parentAcpAgentId,
      childModel: null,
      childEffort: null,
      agentLabel: insightRuntime.agentLabel,
      modelLabel: null,
      childWorktreeBranch: null,
      providerSubagentId: sa.provider_subagent_id ?? null,
      createdAtMs: 0,
      sortOrder: sa.last_seq,
      depth: insightDepth,
      parentAgentId: sa.parent_agent_id ?? agentId,
      parentHostId: sa.parent_host_id ?? hostId,
      lineage: sa.lineage && sa.lineage.length > 0 ? sa.lineage : undefined,
    });
  }

  // 4. Emit each managed descendant once with its timeline call ID, prompt, and lineage.
  for (const [childId, entry] of managedById) {
    const insight = managedInsights.get(childId);
    const effectiveLineage =
      insight?.lineage && insight.lineage.length > 0 ? insight.lineage : entry.lineage;
    const managedAcpId = entry.agent.acp_agent_id ?? null;
    const managedModel = entry.agent.model?.trim() || null;
    const runtime = formatRuntimeForHost(
      entry.childHostId,
      entry.agent.provider,
      managedAcpId,
      managedModel,
      entry.agent.effort ?? null,
    );
    out.push({
      key: `managed:${entry.childHostId}:${childId}`,
      callId: insight?.id ?? childId,
      callIdInferred: insight !== undefined && reconciled.has(insight),
      handle: entry.agent.handle,
      title: entry.agent.title || insight?.role || "Subagent",
      promptPreview: insight?.prompt_preview || entry.agent.status_label || "",
      ...entry.run,
      isManaged: true,
      childHostId: entry.childHostId,
      remoteHostName: entry.remoteHostName,
      childAgentId: childId,
      childProvider: entry.agent.provider,
      childAcpAgentId: managedAcpId,
      childModel: managedModel,
      childEffort: runtime.effort,
      agentLabel: runtime.agentLabel,
      modelLabel: runtime.modelLabel,
      childWorktreeBranch: entry.agent.worktree_branch ?? null,
      providerSubagentId: null,
      createdAtMs: parseTimestampMs(entry.agent.created_at_rfc3339),
      sortOrder: Math.max(
        insight?.last_seq ?? 0,
        entry.agent.latest_seq,
        parseTimestampMs(entry.agent.updated_at_rfc3339),
      ),
      depth: entry.depth,
      parentAgentId: entry.parentAgentId,
      parentHostId: entry.parentHostId,
      lineage: effectiveLineage,
    });
  }

  // 5. Append unconsumed provider native subagents (freshly arrived before stage delta)
  for (const ps of providerList) {
    if (consumedProviderIds.has(ps.subagent_id)) continue;
    const nativeModel = inferNativeSubagentModel(parentProvider, ps);
    const runtime = formatRuntimeForHost(hostId, parentProvider, parentAcpAgentId, nativeModel, null);
    out.push({
      key: `provider:${ps.subagent_id}`,
      callId: `provider:${ps.subagent_id}`,
      title: ps.title?.trim() || "Subagent",
      promptPreview: ps.subtitle || "",
      status: mapProviderSubagentStatus(ps.status),
      isManaged: false,
      childHostId: hostId,
      remoteHostName: null,
      childAgentId: null,
      childProvider: parentProvider,
      childAcpAgentId: parentAcpAgentId,
      childModel: nativeModel,
      childEffort: runtime.effort,
      agentLabel: runtime.agentLabel,
      modelLabel: runtime.modelLabel,
      childWorktreeBranch: null,
      providerSubagentId: ps.subagent_id,
      createdAtMs: 0,
      sortOrder: parseTimestampMs(ps.updated_at),
      depth: 1,
      parentAgentId: agentId,
      parentHostId: hostId,
    });
  }

  // Deterministic ordering:
  // - Flat 1-level lists sort purely by status priority then recency.
  // - Multi-level trees group descendants directly beneath their parent in DFS pre-order.
  const compareItems = (a: ResolvedSubagentItem, b: ResolvedSubagentItem) => {
    const rankDiff = STATUS_PRIORITY[a.status] - STATUS_PRIORITY[b.status];
    if (rankDiff !== 0) return rankDiff;
    return b.sortOrder - a.sortOrder;
  };

  const hasNested = out.some((item) => (item.depth ?? 1) > 1);
  if (!hasNested) {
    out.sort(compareItems);
    return out;
  }

  const knownNodeIds = new Set(out.map((item) => item.childAgentId).filter((id): id is string => Boolean(id)));
  const grouped = new Map<string, ResolvedSubagentItem[]>();
  const roots: ResolvedSubagentItem[] = [];
  for (const item of out) {
    const pId = item.parentAgentId;
    if ((item.depth ?? 1) > 1 && pId && knownNodeIds.has(pId)) {
      const bucket = grouped.get(pId);
      if (bucket) bucket.push(item);
      else grouped.set(pId, [item]);
    } else {
      roots.push(item);
    }
  }
  roots.sort(compareItems);
  for (const bucket of grouped.values()) {
    bucket.sort(compareItems);
  }

  const ordered: ResolvedSubagentItem[] = [];
  const emitted = new Set<string>();
  const emitSubtree = (item: ResolvedSubagentItem) => {
    if (emitted.has(item.key)) return;
    emitted.add(item.key);
    ordered.push(item);
    if (item.childAgentId) {
      for (const child of grouped.get(item.childAgentId) ?? []) {
        emitSubtree(child);
      }
    }
  };
  for (const root of roots) {
    emitSubtree(root);
  }
  // Safety net for any item disconnected from roots
  for (const item of out) {
    if (!emitted.has(item.key)) ordered.push(item);
  }
  return ordered;
}

/**
 * Returns true when a subagent is currently active (running or awaiting approval).
 * Active subagents must never be hidden by historical archive entries (for instance
 * when a previously finished & archived subagent is resumed via `send_message`).
 */
export function isSubagentActiveStatus(status: ResolvedSubagentStatus): boolean {
  return status === "running" || status === "awaiting_permission";
}

/**
 * Filters a resolved session subagent list for UI presentation, keeping all active
 * subagents visible regardless of `archivedSubagents` while hiding archived settled ones.
 */
export function filterVisibleSubagents<
  T extends Pick<
    ResolvedSubagentItem,
    "callId" | "providerSubagentId" | "childAgentId" | "childHostId" | "status"
  >,
>(
  subagents: readonly T[],
  archivedSubagents: Readonly<Record<string, true>>,
  hostId: string | null,
  agentId: string | null,
  archivedThreads?: Readonly<Record<string, unknown>>,
): T[] {
  if (!hostId || !agentId) return [];
  const prefix = `${hostId}::${agentId}::`;
  return subagents.filter((sa) => {
    if (isSubagentActiveStatus(sa.status)) return true;
    if (sa.callId && archivedSubagents[`${prefix}${sa.callId}`]) return false;
    if (sa.providerSubagentId && archivedSubagents[`${prefix}${sa.providerSubagentId}`]) {
      return false;
    }
    if (sa.childAgentId && archivedSubagents[`${prefix}${sa.childAgentId}`]) return false;
    if (
      archivedThreads &&
      sa.childAgentId &&
      archivedThreads[`${sa.childHostId}::${sa.childAgentId}`]
    ) {
      return false;
    }
    return true;
  });
}

export interface ResolvedToolCallSubagent {
  linked: boolean;
  subagent: ResolvedSubagentItem;
  action: "dispatch" | "message";
}

export interface SubagentMessageCallInfo {
  callId: string;
  recipient: string;
  message: string;
  status: "running" | "completed" | "failed" | "denied";
}

/**
 * Extracts structured details from a generic inter-agent message tool call (`send_message`,
 * `send_agent_prompt`, `message_peer`) so it can be surfaced cleanly outside folded tool groups
 * even when the recipient is the parent agent rather than a child subagent.
 */
export function extractSubagentMessageCall(
  item: Extract<TimelineItemView, { item_type: "tool_call" }>,
): SubagentMessageCallInfo | null {
  if (item.detail.kind !== "generic") return null;
  if (classifyGenericSubagentTool(item.detail.tool_name) !== "message") return null;
  const args = tryParseJsonObject(item.detail.arguments_json);
  const recipient = readStringField(args, [
    "handle",
    "Recipient",
    "recipient",
    "agent_id",
    "conversation_id",
    "conversationId",
  ]);
  const message = readStringField(args, ["Message", "message", "prompt", "content", "text"]);
  return {
    callId: item.call_id || `msg-${item.seq}`,
    recipient,
    message,
    status: item.status,
  };
}

/** Structural value-equality check to keep `sessionSubagents` referentially stable across unrelated host ticks. */
export function areResolvedSubagentsEqual(
  a: readonly ResolvedSubagentItem[],
  b: readonly ResolvedSubagentItem[],
): boolean {
  if (a === b) return true;
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i += 1) {
    const x = a[i];
    const y = b[i];
    if (
      x.key !== y.key ||
      x.callId !== y.callId ||
      x.callIdInferred !== y.callIdInferred ||
      x.handle !== y.handle ||
      x.createdAtMs !== y.createdAtMs ||
      x.activeStartedAtMs !== y.activeStartedAtMs ||
      x.sortOrder !== y.sortOrder ||
      x.title !== y.title ||
      x.status !== y.status ||
      x.childAgentId !== y.childAgentId ||
      x.childProvider !== y.childProvider ||
      (x.childAcpAgentId ?? null) !== (y.childAcpAgentId ?? null) ||
      (x.childModel ?? null) !== (y.childModel ?? null) ||
      (x.childEffort ?? null) !== (y.childEffort ?? null) ||
      (x.agentLabel ?? null) !== (y.agentLabel ?? null) ||
      (x.modelLabel ?? null) !== (y.modelLabel ?? null) ||
      (x.childWorktreeBranch ?? null) !== (y.childWorktreeBranch ?? null) ||
      x.providerSubagentId !== y.providerSubagentId ||
      x.childHostId !== y.childHostId ||
      x.promptPreview !== y.promptPreview ||
      (x.depth ?? 1) !== (y.depth ?? 1) ||
      (x.parentAgentId ?? null) !== (y.parentAgentId ?? null) ||
      (x.parentHostId ?? null) !== (y.parentHostId ?? null) ||
      (x.lineage?.length ?? 0) !== (y.lineage?.length ?? 0)
    ) {
      return false;
    }
  }
  return true;
}

function isManagedDispatchTool(name: string | undefined): boolean {
  const clean = name?.trim().toLowerCase().replace(/^run\s+/, "").replace(/[_\s/:.]+/g, " ");
  return /^(?:(?:mcp )?orbit )?(?:create agent|spawn workspace peer)$/.test(clean ?? "");
}

export function classifyGenericSubagentTool(toolName: string): "dispatch" | "message" | null {
  const stripped = toolName.trim().replace(/^run\s+/i, "");
  if (isManagedDispatchTool(toolName) || /^(?:invoke|spawn)\s+subagent$/i.test(stripped)) return "dispatch";
  if (!stripped) return null;
  const firstToken = (stripped.split(/\s+/)[0] ?? "").toLowerCase();
  if (!firstToken) return null;
  const leaf = firstToken.split(/[:/.]|__/).pop() ?? firstToken;
  if (leaf === "invoke_subagent" || leaf === "spawn_subagent" || isManagedDispatchTool(leaf)) {
    return "dispatch";
  }
  if (
    leaf === "send_message" ||
    leaf === "sendmessage" ||
    leaf === "send_agent_prompt" ||
    leaf === "message_peer" ||
    leaf === "mcp_orbit_send_agent_prompt" ||
    leaf === "mcp_orbit_message_peer"
  ) {
    return "message";
  }
  return null;
}

function tryParseJsonObject(raw: string): Record<string, unknown> | null {
  const trimmed = raw.trim();
  if (!trimmed || !trimmed.startsWith("{")) return null;
  try {
    const parsed: unknown = JSON.parse(trimmed);
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

function readStringField(obj: Record<string, unknown> | null, keys: readonly string[]): string {
  if (!obj) return "";
  for (const key of keys) {
    const val = obj[key];
    if (typeof val === "string" && val.trim().length > 0) {
      return val.trim();
    }
  }
  return "";
}

type DispatchToolItem = Extract<TimelineItemView, { item_type: "tool_call" }> & {
  output_preview?: string | null;
};

function resultField(value: unknown, keys: readonly string[]): string {
  if (typeof value === "string") {
    try {
      return resultField(JSON.parse(value), keys);
    } catch {
      // Historical previews can include a prose prefix or end mid-object.
      for (const key of keys) {
        const match = value.match(new RegExp(`"${key}"\\s*:\\s*("(?:\\\\.|[^"\\\\])*")`));
        if (match) {
          try { return JSON.parse(match[1]).trim(); } catch { continue; }
        }
      }
    }
  } else if (Array.isArray(value)) {
    for (const block of value) {
      const field = resultField(block, keys);
      if (field) return field;
    }
  } else if (value && typeof value === "object") {
    const obj = value as Record<string, unknown>;
    const direct = readStringField(obj, keys);
    if (direct) return direct;
    for (const key of ["structuredContent", "content", "result", "output", "text"]) {
      const field = resultField(obj[key], keys);
      if (field) return field;
    }
  }
  return "";
}

/**
 * Resolves a subagent dispatch (`invoke_subagent`, `create_agent`, `spawn_workspace_peer`, or
 * `detail.kind === "subagent"`) or follow-up messaging (`send_message`, `send_agent_prompt`,
 * `message_peer`) tool call against the session's unified `ResolvedSubagentItem` list.
 */
function resolveExactToolCall(
  item: DispatchToolItem,
  subagents: readonly ResolvedSubagentItem[],
  fallbackHostId?: string,
  matchNames = true,
): ResolvedToolCallSubagent | null {
  const matchById = (id: string | null | undefined): ResolvedSubagentItem | undefined => {
    const clean = id?.trim();
    if (!clean) return undefined;
    return subagents.find(
      (sa) =>
        sa.childAgentId === clean ||
        sa.providerSubagentId === clean ||
        sa.callId === clean,
    );
  };

  const matchByTitle = (title: string, candidates = subagents) => {
    const normal = normalizeSubagentName(title);
    if (!matchNames || !normal || normal === "subagent") return undefined;
    const matches = candidates.filter((sa) =>
      normalizeSubagentName(sa.title) === normal || normalizeSubagentName(sa.handle ?? "") === normal,
    );
    return matches.length === 1 ? matches[0] : undefined;
  };
  const matchByCall = (candidates: readonly ResolvedSubagentItem[]) =>
    candidates.find((sa) => sa.callId === item.call_id && (matchNames || !sa.callIdInferred));

  let title: string;
  let prompt: string;
  let childId: string | null;
  let managed = false;
  let matched: ResolvedSubagentItem | undefined;
  if (item.detail.kind === "subagent") {
    title = item.detail.role.trim();
    prompt = item.detail.prompt_preview;
    childId = item.detail.child_agent_id;
    managed = Boolean(childId?.startsWith("ag_")) || isManagedDispatchTool(item.summary_title);
    if (!managed && !childId && !prompt.trim() && title.toLowerCase() === "subagent" && item.status !== "running") {
      return null;
    }
    if (childId) {
      matched = managed ? subagents.find((sa) => sa.isManaged && sa.childAgentId === childId) : matchById(childId);
    } else {
      const cards = subagents.filter((sa) =>
        sa.isManaged || (!managed && (sa.childAgentId || sa.providerSubagentId)),
      );
      matched = matchByCall(cards);
      if (!matched && (item.status === "running" || item.status === "completed") &&
          !GENERIC_SUBAGENT_TITLES.has(normalizeSubagentName(title))) {
        matched = matchByTitle(title, cards);
      }
    }
    if (!managed && !matched) matched = matchByCall(subagents);
  } else if (item.detail.kind === "generic") {
    const name = item.detail.tool_name;
    const action = classifyGenericSubagentTool(name);
    if (!action) return null;
    const args = tryParseJsonObject(item.detail.arguments_json ?? "");
    if (action === "message") {
      const recipient = readStringField(args, ["Recipient", "recipient", "agent_id", "conversation_id", "conversationId", "child_agent_id"]);
      matched = matchById(recipient) ?? matchByTitle(recipient) ?? matchByTitle(readStringField(args, ["Role", "role", "title", "handle"]));
      return matched ? { subagent: matched, action, linked: true } : null;
    }

    const titles = ["title", "handle", "toolSummary", "Role", "role", "TypeName", "type_name"];
    const ids = ["child_agent_id", "agent_id"];
    const rawArgs = item.detail.arguments_json;
    const argumentField = (keys: readonly string[]) =>
      args ? readStringField(args, keys) : resultField(rawArgs, keys);
    title = resultField(item.output_preview, titles) || argumentField(titles);
    prompt = argumentField(["initial_prompt", "Prompt", "prompt", "brief", "task"]);
    managed = isManagedDispatchTool(name);
    const targetId = resultField(item.output_preview, ids) || argumentField(ids) ||
      (!managed ? readStringField(args, ["conversation_id", "conversationId", "Recipient", "recipient"]) : "");
    childId = managed && targetId ? targetId : null;
    const candidates = managed ? subagents.filter((sa) => sa.isManaged) : subagents;
    if (targetId) {
      matched = managed ? candidates.find((sa) => sa.childAgentId === targetId) : matchById(targetId);
    } else {
      matched = matchByCall(candidates) ?? matchByTitle(title, candidates);
      if (!matched && typeof args?.handle === "string") {
        matched = matchByTitle(args.handle, candidates);
      }
    }
  } else {
    return null;
  }

  const callId = item.call_id || `dispatch-${item.seq}`;
  if (matched) {
    return {
      linked: true,
      subagent: { ...matched, callId, promptPreview: prompt || matched.promptPreview },
      action: "dispatch",
    };
  }
  return {
    linked: false,
    action: "dispatch",
    subagent: {
      key: `tool:${callId}`,
      callId,
      title: title || "Subagent",
      promptPreview: prompt,
      status: managed && item.status === "completed" ? "running" : mapInsightStatus(item.status),
      isManaged: managed,
      childHostId: fallbackHostId ?? "",
      remoteHostName: null,
      childAgentId: childId,
      childProvider: null,
      providerSubagentId: null,
      createdAtMs: 0,
      sortOrder: item.seq,
    },
  };
}

/** Reserve exact identities before assigning anonymous launches across the loaded timeline. */
export function resolveDelegationDispatches(
  items: readonly DispatchToolItem[],
  sessionSubagents: readonly ResolvedSubagentItem[],
  fallbackHostId?: string,
  messagePeers = sessionSubagents,
) {
  const resolved = new Map<DispatchToolItem, ResolvedToolCallSubagent>();
  const claimCounts = new Map<string, number>();
  for (const item of items) {
    const recipients = messagePeers !== sessionSubagents && item.detail.kind === "generic" &&
      classifyGenericSubagentTool(item.detail.tool_name) === "message"
      ? messagePeers : sessionSubagents;
    const match = resolveExactToolCall(item, recipients, fallbackHostId);
    if (!match) continue;
    resolved.set(item, match);
    if (match.action === "dispatch" && match.linked) {
      const key = match.subagent.key;
      claimCounts.set(key, (claimCounts.get(key) ?? 0) + 1);
    }
  }

  const claimed = new Set<string>();
  const pending: { item: DispatchToolItem; fallback: ResolvedSubagentItem; timestamp: number }[] = [];
  for (const [item, initial] of resolved) {
    if (initial.action !== "dispatch") continue;
    // Multiple launches can request the same title. Only IDs are authoritative
    // in a conflict; return name-only claims to the chronological pass.
    let match = initial;
    if (initial.linked && (claimCounts.get(initial.subagent.key) ?? 0) > 1) {
      match = resolveExactToolCall(item, sessionSubagents, fallbackHostId, false)!;
      resolved.set(item, match);
    }
    if (match.linked) {
      claimed.add(match.subagent.key);
    } else if (
      match.subagent.isManaged &&
      !match.subagent.childAgentId &&
      item.timestamp_ms != null &&
      (item.status === "completed" || item.status === "running")
    ) {
      pending.push({ item, fallback: match.subagent, timestamp: item.timestamp_ms });
    }
  }

  if (!pending.length) return resolved;
  const candidates = sessionSubagents.filter((sa) => sa.isManaged && !claimed.has(sa.key));
  const edges = pending.flatMap((launch) => candidates.flatMap((card) => {
    const delta = card.createdAtMs - launch.timestamp;
    if (card.createdAtMs > 0 && delta >= -5_000 && delta <= 30_000) {
      return [{ launch, card, distance: Math.abs(delta) }];
    }
    // Older snapshots only have last activity. Require a single anonymous
    // launch and a single unattached card with exactly the same clock reading.
    if (!card.createdAtMs && card.callId === card.childAgentId &&
        pending.length === 1 && candidates.length === 1 &&
        launch.fallback.title === "Subagent" && card.sortOrder === launch.timestamp) {
      return [{ launch, card, distance: 0 }];
    }
    return [];
  }));
  edges.sort((a, b) => a.distance - b.distance || a.launch.timestamp - b.launch.timestamp ||
    a.launch.item.seq - b.launch.item.seq || a.launch.item.call_id.localeCompare(b.launch.item.call_id) ||
    a.card.key.localeCompare(b.card.key));

  const assigned = new Set<DispatchToolItem>();
  for (const { launch: { item, fallback }, card } of edges) {
    if (assigned.has(item) || claimed.has(card.key)) continue;
    assigned.add(item);
    claimed.add(card.key);
    resolved.set(item, {
      action: "dispatch",
      linked: true,
      subagent: { ...card, callId: fallback.callId, promptPreview: fallback.promptPreview || card.promptPreview },
    });
  }
  return resolved;
}

export function resolveToolCallSubagent(
  item: DispatchToolItem,
  subagents: readonly ResolvedSubagentItem[],
  fallbackHostId?: string,
): ResolvedToolCallSubagent | null {
  if (!subagents.length) return resolveExactToolCall(item, subagents, fallbackHostId);
  return resolveDelegationDispatches([item], subagents, fallbackHostId).get(item) ?? null;
}

const GENERIC_SUBAGENT_TITLES = new Set([
  "subagent",
  "agent",
  "research",
  "self",
  "task",
  "worker",
  "assistant",
  "default",
  "helper",
  "peer",
]);

function escapeRegExp(raw: string): string {
  return raw.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function isAsciiWordChar(ch: string | undefined): boolean {
  return Boolean(ch && /[A-Za-z0-9_]/.test(ch));
}

export type SubagentTextSegment =
  | { kind: "text"; text: string }
  | { kind: "subagent"; text: string; subagent: ResolvedSubagentItem };

export interface SubagentNameMatcher {
  matchExact: (text: string) => ResolvedSubagentItem | null;
  splitText: (text: string) => SubagentTextSegment[];
}

/**
 * Compiles a memoizable longest-first matcher for known session subagent names so assistant
 * markdown prose can turn exact subagent references into clickable links with minimal overhead.
 */
export function buildSubagentNameMatcher(
  subagents: readonly ResolvedSubagentItem[] | undefined,
): SubagentNameMatcher | null {
  if (!subagents || subagents.length === 0) return null;

  const byLower = new Map<string, ResolvedSubagentItem>();
  for (const sa of subagents) {
    const trimmed = sa.title.trim();
    if (trimmed.length < 3) continue;
    const lower = trimmed.toLowerCase();
    if (GENERIC_SUBAGENT_TITLES.has(lower)) continue;
    if (!byLower.has(lower)) {
      byLower.set(lower, sa);
    }
  }

  if (byLower.size === 0) return null;

  const entries = Array.from(byLower.entries()).sort((a, b) => b[0].length - a[0].length);
  const pattern = entries.map(([lower]) => escapeRegExp(lower)).join("|");
  const regex = new RegExp(pattern, "gi");

  const matchExact = (raw: string): ResolvedSubagentItem | null => {
    const trimmed = raw.trim().replace(/^@+/, "").toLowerCase();
    if (!trimmed) return null;
    return byLower.get(trimmed) ?? null;
  };

  const splitText = (text: string): SubagentTextSegment[] => {
    if (!text) return [{ kind: "text", text }];
    regex.lastIndex = 0;
    const out: SubagentTextSegment[] = [];
    let cursor = 0;
    let match: RegExpExecArray | null = null;

    while ((match = regex.exec(text)) !== null) {
      const matchedText = match[0];
      const start = match.index;
      const end = start + matchedText.length;

      const firstChar = matchedText[0];
      const lastChar = matchedText[matchedText.length - 1];
      const prevChar = start > 0 ? text[start - 1] : undefined;
      const nextChar = end < text.length ? text[end] : undefined;

      // ASCII word boundary guard (avoid matching inside identifiers or file paths while staying CJK-safe)
      if (
        isAsciiWordChar(firstChar) &&
        (isAsciiWordChar(prevChar) || prevChar === "/" || prevChar === "-" || prevChar === ".")
      ) {
        continue;
      }
      if (
        isAsciiWordChar(lastChar) &&
        (isAsciiWordChar(nextChar) ||
          nextChar === "/" ||
          nextChar === "-" ||
          (nextChar === "." && isAsciiWordChar(text[end + 1])))
      ) {
        continue;
      }

      const sa = byLower.get(matchedText.toLowerCase());
      if (!sa) continue;

      if (start > cursor) {
        out.push({ kind: "text", text: text.slice(cursor, start) });
      }
      out.push({ kind: "subagent", text: matchedText, subagent: sa });
      cursor = end;
    }

    if (out.length === 0) {
      return [{ kind: "text", text }];
    }
    if (cursor < text.length) {
      out.push({ kind: "text", text: text.slice(cursor) });
    }
    return out;
  };

  return { matchExact, splitText };
}
