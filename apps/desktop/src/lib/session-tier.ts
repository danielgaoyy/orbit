import type { AgentCardView } from "./orbit-ipc.ts";

export type SessionTier = "conversation" | "subagent" | "personal_router" | "automation";

/**
 * Resolve canonical session sovereignty tier matching `orbit_protocol::resolve_session_tier`.
 * Ensures cross-host subagents and orphaned subagents remain hidden while Router-dispatched
 * conversations remain visible across Sidebar, Overview, and Workspace panels.
 */
export function resolveAgentSessionTier(
  agent: Pick<AgentCardView, "background" | "parent_agent_id" | "labels">,
): SessionTier {
  if (agent.background) return "automation";
  const labels = agent.labels;
  if (labels?.["orbit.role"] === "personal_router") return "personal_router";
  const explicit = labels?.["orbit.session_tier"];
  if (
    explicit === "conversation" ||
    explicit === "subagent" ||
    explicit === "personal_router" ||
    explicit === "automation"
  ) {
    return explicit;
  }
  if (labels?.["orbit.routed_by_agent"]) return "conversation";
  if (agent.parent_agent_id) return "subagent";
  return "conversation";
}

export function isSubagentCard(
  agent: Pick<AgentCardView, "background" | "parent_agent_id" | "labels">,
): boolean {
  return resolveAgentSessionTier(agent) === "subagent";
}

export function isRoutedConversationCard(
  agent: Pick<AgentCardView, "labels">,
): boolean {
  return Boolean(agent.labels?.["orbit.routed_by_agent"]?.trim());
}
