import type {
  AgentCardView,
  HostCardView,
  HostDetailView,
  ProviderKind,
  ProviderSubagentDescriptor,
  SessionSubagentEntryView,
} from "@/lib/orbit-ipc";

export type ResolvedSubagentStatus = "awaiting_permission" | "running" | "failed" | "completed";

export interface ResolvedSubagentItem {
  /** Stable unique key for React lists */
  key: string;
  /** Call ID passed to openSubagentTab */
  callId: string;
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
  /** Provider kind of the managed child agent (when isManaged === true) */
  childProvider: ProviderKind | null;
  /** Provider native subagent ID when read-only native subagent */
  providerSubagentId: string | null;
  /** Sort tie-breaker (higher = more recent) */
  sortOrder: number;
}

const STATUS_PRIORITY: Record<ResolvedSubagentStatus, number> = {
  awaiting_permission: 0,
  running: 1,
  failed: 2,
  completed: 3,
};

function mapManagedStatus(agent: AgentCardView, fallbackStatus?: string): ResolvedSubagentStatus {
  if (agent.is_awaiting_permission) return "awaiting_permission";
  if (agent.is_running || agent.has_background_tasks) return "running";
  if (
    fallbackStatus === "failed" ||
    fallbackStatus === "denied" ||
    agent.status_label.toLowerCase().includes("error")
  ) {
    return "failed";
  }
  return "completed";
}

function mapProviderSubagentStatus(
  status: ProviderSubagentDescriptor["status"],
): ResolvedSubagentStatus {
  if (status === "running") return "running";
  if (status === "completed") return "completed";
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

/**
 * Resolves a single unified, deduplicated list of subagents for a parent conversation,
 * combining:
 * 1. Rust-projected `stage.insights.subagents` (timeline calls + local managed + provider native)
 * 2. Orbit managed child agents across ALL hosts (including cross-host delegations)
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

  const hostNameById = new Map<string, string>();
  for (const h of fleetHosts) {
    hostNameById.set(h.host_id, h.display_name || h.host_id);
  }
  for (const [hid, detail] of Object.entries(hostDetails)) {
    if (!hostNameById.has(hid)) {
      hostNameById.set(hid, detail.host.display_name || hid);
    }
  }

  // 1. Index all Orbit managed children (across all hosts) and archived agent IDs
  const managedById = new Map<
    string,
    { childHostId: string; remoteHostName: string | null; agent: AgentCardView }
  >();
  const archivedManagedIds = new Set<string>();

  for (const [hid, detail] of Object.entries(hostDetails)) {
    for (const ag of detail.agents) {
      const matchesParent =
        ag.parent_agent_id === agentId &&
        (!ag.parent_host_id || ag.parent_host_id === hostId);
      if (!matchesParent) continue;

      if (ag.archived_at_rfc3339) {
        archivedManagedIds.add(ag.agent_id);
        continue;
      }

      managedById.set(ag.agent_id, {
        childHostId: hid,
        remoteHostName: hid !== hostId ? (hostNameById.get(hid) ?? hid) : null,
        agent: ag,
      });
    }
  }

  // 2. Index live provider native subagents on the parent host
  const providerList = hostDetails[hostId]?.provider_subagents?.[agentId] ?? [];
  const providerById = new Map<string, ProviderSubagentDescriptor>();
  for (const ps of providerList) {
    providerById.set(ps.subagent_id, ps);
  }

  const consumedManagedIds = new Set<string>();
  const consumedProviderIds = new Set<string>();
  const out: ResolvedSubagentItem[] = [];

  // 3. Walk Rust-projected insights first to preserve timeline prompt previews & call IDs
  for (const sa of insightsSubagents) {
    if (sa.child_agent_id && archivedManagedIds.has(sa.child_agent_id)) {
      continue;
    }

    const managed = sa.child_agent_id ? managedById.get(sa.child_agent_id) : undefined;
    if (managed) {
      consumedManagedIds.add(managed.agent.agent_id);
      out.push({
        key: `managed:${managed.childHostId}:${managed.agent.agent_id}`,
        callId: sa.id,
        title: managed.agent.title || sa.role || "Subagent",
        promptPreview: sa.prompt_preview || managed.agent.status_label || "",
        status: mapManagedStatus(managed.agent, sa.status),
        isManaged: true,
        childHostId: managed.childHostId,
        remoteHostName: managed.remoteHostName,
        childAgentId: managed.agent.agent_id,
        childProvider: managed.agent.provider,
        providerSubagentId: null,
        sortOrder: Math.max(
          sa.last_seq,
          managed.agent.latest_seq,
          parseTimestampMs(managed.agent.updated_at_rfc3339),
        ),
      });
      continue;
    }

    const providerSub = sa.provider_subagent_id
      ? providerById.get(sa.provider_subagent_id)
      : undefined;
    if (providerSub) {
      consumedProviderIds.add(providerSub.subagent_id);
      out.push({
        key: `provider:${providerSub.subagent_id}`,
        callId: sa.id,
        title: providerSub.title?.trim() || sa.role || "Subagent",
        promptPreview: sa.prompt_preview || providerSub.subtitle || "",
        status: mapProviderSubagentStatus(providerSub.status),
        isManaged: false,
        childHostId: hostId,
        remoteHostName: null,
        childAgentId: null,
        childProvider: null,
        providerSubagentId: providerSub.subagent_id,
        sortOrder: Math.max(sa.last_seq, parseTimestampMs(providerSub.updated_at)),
      });
      continue;
    }

    out.push({
      key: `insight:${sa.id}`,
      callId: sa.id,
      title: sa.role || "Subagent",
      promptPreview: sa.prompt_preview || "",
      status: mapInsightStatus(sa.status),
      isManaged: false,
      childHostId: hostId,
      remoteHostName: null,
      childAgentId: null,
      childProvider: null,
      providerSubagentId: sa.provider_subagent_id ?? null,
      sortOrder: sa.last_seq,
    });
  }

  // 4. Append unconsumed Orbit managed children (cross-host or freshly spawned before stage delta)
  for (const [childId, entry] of managedById) {
    if (consumedManagedIds.has(childId)) continue;
    out.push({
      key: `managed:${entry.childHostId}:${childId}`,
      callId: childId,
      title: entry.agent.title || "Subagent",
      promptPreview: entry.agent.status_label || "",
      status: mapManagedStatus(entry.agent),
      isManaged: true,
      childHostId: entry.childHostId,
      remoteHostName: entry.remoteHostName,
      childAgentId: childId,
      childProvider: entry.agent.provider,
      providerSubagentId: null,
      sortOrder: Math.max(
        entry.agent.latest_seq,
        parseTimestampMs(entry.agent.updated_at_rfc3339),
      ),
    });
  }

  // 5. Append unconsumed provider native subagents (freshly arrived before stage delta)
  for (const ps of providerList) {
    if (consumedProviderIds.has(ps.subagent_id)) continue;
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
      childProvider: null,
      providerSubagentId: ps.subagent_id,
      sortOrder: parseTimestampMs(ps.updated_at),
    });
  }

  // Deterministic ordering: awaiting_permission -> running -> failed -> completed, newest first
  out.sort((a, b) => {
    const rankDiff = STATUS_PRIORITY[a.status] - STATUS_PRIORITY[b.status];
    if (rankDiff !== 0) return rankDiff;
    return b.sortOrder - a.sortOrder;
  });

  return out;
}
