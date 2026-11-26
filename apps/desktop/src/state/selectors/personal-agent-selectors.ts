import type { AgentCardView, HostCardView, HostDetailView } from "../../lib/orbit-ipc.ts";
import type { ArchivedThreadEntry, OrbitUiState } from "../store-types.ts";

export interface PersonalAgentRef {
  readonly hostId: string;
  readonly hostName: string;
  readonly agentId: string;
  readonly agent: AgentCardView;
}

export type PersonalAgentSelectorState = Pick<
  OrbitUiState,
  "fleet" | "hostDetails" | "archivedThreads"
> & {
  primaryRouterHostId?: string | null;
};

export const PERSONAL_AGENT_ROLE_LABEL_KEY = "orbit.role";
export const PERSONAL_AGENT_ROLE_VALUE = "personal_router";

const DEFAULT_PERSONAL_AGENT_TITLE_RE =
  /^(?:personal\s+(?:router|agent)|@?router|个人代理|パーソナルエージェント)$/i;

/**
 * Returns true when a Personal Agent conversation's raw title is empty or a legacy/default
 * placeholder (`"Personal Router"`, `"Personal Agent"`, `"router"`, etc.) and should render
 * using the localized `sidebar.personalRouter` label.
 */
export function isDefaultPersonalAgentTitle(title: string | null | undefined): boolean {
  const trimmed = title?.trim();
  if (!trimmed) return true;
  return DEFAULT_PERSONAL_AGENT_TITLE_RE.test(trimmed);
}

function isPersonalRouterRoleSlug(raw: string | undefined): boolean {
  if (!raw) return false;
  const norm = raw.trim().toLowerCase().replace(/-/g, "_");
  return norm === "personal_router" || norm === "router" || norm === "chief_of_staff";
}

function isHostOnline(host: Pick<HostCardView, "connection_state"> | undefined): boolean {
  return (
    host?.connection_state === "online_direct" || host?.connection_state === "online_relay"
  );
}

function parseUpdatedAtMs(rfc3339: string | null | undefined): number {
  if (!rfc3339) return 0;
  const parsed = Date.parse(rfc3339);
  return Number.isFinite(parsed) ? parsed : 0;
}

/**
 * Returns identification rank for a candidate Personal Agent card (lower = higher priority),
 * or `null` if the agent is not a Personal Agent.
 *
 * Mirrors `crates/orbit-client-core/src/personal_agent.rs::personal_agent_match_tier`:
 *   0: `labels["orbit.role"]` is `personal_router` / `router` / `chief_of_staff`
 *   1: `handle` equals `"router"` (case-insensitive)
 *   2: `title` contains `"Personal Router"` (case-insensitive)
 */
export function personalAgentMatchRank(
  agent: Pick<AgentCardView, "labels" | "handle" | "title">,
): 0 | 1 | 2 | null {
  if (isPersonalRouterRoleSlug(agent.labels?.[PERSONAL_AGENT_ROLE_LABEL_KEY])) {
    return 0;
  }
  if (agent.handle?.trim().toLowerCase() === "router") {
    return 1;
  }
  if (agent.title.toLowerCase().includes("personal router")) {
    return 2;
  }
  return null;
}

export function isPersonalAgentCard(
  agent: Pick<AgentCardView, "labels" | "handle" | "title" | "archived_at_rfc3339"> | null | undefined,
): boolean {
  if (!agent || agent.archived_at_rfc3339) return false;
  return personalAgentMatchRank(agent) !== null;
}

const refCacheByAgent = new WeakMap<AgentCardView, PersonalAgentRef>();

function getStablePersonalAgentRef(
  hostId: string,
  hostName: string,
  agent: AgentCardView,
): PersonalAgentRef {
  const cached = refCacheByAgent.get(agent);
  if (cached && cached.hostId === hostId && cached.hostName === hostName && cached.agentId === agent.agent_id) {
    return cached;
  }
  const created: PersonalAgentRef = Object.freeze({
    hostId,
    hostName,
    agentId: agent.agent_id,
    agent,
  });
  refCacheByAgent.set(agent, created);
  return created;
}

function isThreadArchived(
  archivedThreads: Readonly<Record<string, ArchivedThreadEntry>> | undefined,
  hostId: string,
  agent: Pick<AgentCardView, "agent_id" | "archived_at_rfc3339">,
): boolean {
  if (agent.archived_at_rfc3339) return true;
  return Boolean(archivedThreads?.[`${hostId}::${agent.agent_id}`]);
}

/**
 * Single source of truth for locating the active Personal Agent across the multi-host fleet.
 *
 * Mirrors `crates/orbit-client-core/src/personal_agent.rs` tie-breaker order:
 *   1. Match tier (`orbit.role` > `handle == "router"` > title contains `"Personal Router"`)
 *   2. Primary router host (`primaryRouterHostId`)
 *   3. Online host over offline host
 *   4. Most recently updated session (`updated_at_rfc3339` descending)
 *   5. Deterministic lexical `(hostId, agentId)`
 * Archived sessions (daemon or local) are always excluded.
 *
 * Referentially stable via `WeakMap<AgentCardView, PersonalAgentRef>` so calling inside
 * React 19 `useOrbitSelector(selectPersonalAgentRef)` never causes infinite re-renders.
 */
export function selectPersonalAgentRef(
  state: PersonalAgentSelectorState,
  primaryRouterHostIdOverride?: string | null,
): PersonalAgentRef | null {
  const rawPrimary =
    primaryRouterHostIdOverride !== undefined
      ? primaryRouterHostIdOverride
      : (state.primaryRouterHostId ?? null);
  const primaryHostId = rawPrimary?.trim() ? rawPrimary.trim() : null;

  const hosts: readonly HostCardView[] = state.fleet.hosts;
  const hostDetails: Readonly<Record<string, HostDetailView>> = state.hostDetails;
  const archivedThreads = state.archivedThreads;

  const entries: Array<{
    hostId: string;
    hostName: string;
    detail: HostDetailView;
    isPrimary: boolean;
    online: boolean;
  }> = [];
  const seenHostIds = new Set<string>();

  for (const h of hosts) {
    const detail = hostDetails[h.host_id];
    if (!detail) continue;
    seenHostIds.add(h.host_id);
    entries.push({
      hostId: h.host_id,
      hostName: h.display_name || detail.host.display_name || h.host_id,
      detail,
      isPrimary: Boolean(primaryHostId && h.host_id === primaryHostId),
      online: isHostOnline(h),
    });
  }

  for (const [hostId, detail] of Object.entries(hostDetails)) {
    if (seenHostIds.has(hostId) || !detail) continue;
    entries.push({
      hostId,
      hostName: detail.host.display_name || hostId,
      detail,
      isPrimary: Boolean(primaryHostId && hostId === primaryHostId),
      online: isHostOnline(detail.host),
    });
  }

  let best: {
    rank: 0 | 1 | 2;
    notPrimary: boolean;
    offline: boolean;
    updatedAtMs: number;
    hostId: string;
    hostName: string;
    agent: AgentCardView;
  } | null = null;

  for (const entry of entries) {
    for (const agent of entry.detail.agents) {
      if (isThreadArchived(archivedThreads, entry.hostId, agent)) continue;
      const rank = personalAgentMatchRank(agent);
      if (rank === null) continue;

      const candidate = {
        rank,
        notPrimary: primaryHostId !== null && !entry.isPrimary,
        offline: !entry.online,
        updatedAtMs: parseUpdatedAtMs(agent.updated_at_rfc3339),
        hostId: entry.hostId,
        hostName: entry.hostName,
        agent,
      };

      const isBetter =
        best === null ||
        candidate.rank < best.rank ||
        (candidate.rank === best.rank &&
          (Number(candidate.notPrimary) < Number(best.notPrimary) ||
            (candidate.notPrimary === best.notPrimary &&
              (Number(candidate.offline) < Number(best.offline) ||
                (candidate.offline === best.offline &&
                  (candidate.updatedAtMs > best.updatedAtMs ||
                    (candidate.updatedAtMs === best.updatedAtMs &&
                      (candidate.hostId < best.hostId ||
                        (candidate.hostId === best.hostId &&
                          candidate.agent.agent_id < best.agent.agent_id)))))))));

      if (isBetter) {
        best = candidate;
      }
    }
  }

  return best ? getStablePersonalAgentRef(best.hostId, best.hostName, best.agent) : null;
}

/**
 * Primitive boolean selector for checking whether the currently focused conversation
 * in the main stage is the Personal Agent.
 */
export function selectIsSelectedAgentPersonalAgent(
  state: Pick<
    OrbitUiState,
    | "fleet"
    | "hostDetails"
    | "archivedThreads"
    | "primaryRouterHostId"
    | "workspacePanel"
    | "isDraftNewSession"
    | "selectedHostId"
    | "selectedAgentId"
  >,
): boolean {
  if (
    state.workspacePanel !== "conversation" ||
    state.isDraftNewSession ||
    !state.selectedHostId ||
    !state.selectedAgentId
  ) {
    return false;
  }
  const pa = selectPersonalAgentRef(state);
  return Boolean(
    pa && pa.hostId === state.selectedHostId && pa.agentId === state.selectedAgentId,
  );
}

