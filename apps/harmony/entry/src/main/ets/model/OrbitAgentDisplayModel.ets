import {
  AgentCardDto,
  ProviderModelCatalogDto,
  SessionSubagentEntryDto,
  formatEffortLabel,
  formatProviderLabel
} from './OrbitTypes';

export type OrbitAgentListItemVariant = 'row' | 'card';

export function isAgentListItemActiveStatus(status: string): boolean {
  return status === 'running' || status === 'awaiting_permission';
}

export function resolveAgentListStatusResource(status: string): Resource {
  if (status === 'awaiting_permission') {
    return $r('app.string.overview_subagentApprovalRequired');
  }
  if (status === 'running') {
    return $r('app.string.overview_subagentWorking');
  }
  if (status === 'failed') {
    return $r('app.string.overview_subagentFailed');
  }
  if (status === 'canceled') {
    return $r('app.string.overview_subagentStopped');
  }
  if (status === 'completed') {
    return $r('app.string.overview_subagentFinished');
  }
  return $r('app.string.overview_subagentIdle');
}

function isAgentStatusCanceled(agent: AgentCardDto | null | undefined): boolean {
  if (agent === null || agent === undefined) {
    return false;
  }
  const state = agent.status?.state ?? '';
  const lower = (agent.status_label ?? '').toLowerCase();
  return (
    state === 'closed' ||
    lower.includes('cancel') ||
    lower.includes('interrupt') ||
    lower.includes('stopped') ||
    lower.includes('closed')
  );
}

function isAgentStatusFailed(agent: AgentCardDto | null | undefined): boolean {
  if (agent === null || agent === undefined) {
    return false;
  }
  const state = agent.status?.state ?? '';
  const lower = (agent.status_label ?? '').toLowerCase();
  return state === 'error' || lower.includes('error') || lower.includes('fail');
}

export function mapAgentCardToListStatus(agent: AgentCardDto): string {
  const state = agent.status?.state ?? '';
  if (agent.is_awaiting_permission || state === 'awaiting_permission') {
    return 'awaiting_permission';
  }
  if (
    (agent.is_running && agent.has_background_tasks !== true) ||
    state === 'initializing' ||
    state === 'running' ||
    (state === 'autonomous_wakeup' && agent.has_background_tasks !== true)
  ) {
    return 'running';
  }
  if (isAgentStatusFailed(agent)) {
    return 'failed';
  }
  if (isAgentStatusCanceled(agent)) {
    return 'canceled';
  }
  return 'idle';
}

export function mapBriefingStateToListStatus(
  state: string,
  liveAgent?: AgentCardDto | null
): string {
  if (state === 'awaiting') {
    return 'awaiting_permission';
  }
  if (state === 'running') {
    return 'running';
  }
  if (state === 'error') {
    return 'failed';
  }
  if (state === 'done') {
    return isAgentStatusCanceled(liveAgent) ? 'canceled' : 'completed';
  }
  return 'idle';
}

export function resolveCompactAgentModelLabel(
  agent: AgentCardDto | null | undefined,
  catalogs: ProviderModelCatalogDto[]
): string {
  if (agent === null || agent === undefined) {
    return '';
  }
  const rawModel = (agent.model ?? '').trim();
  if (rawModel.length === 0 || rawModel.toLowerCase() === 'default') {
    return '';
  }
  const provider = agent.provider.trim().toLowerCase();
  if (provider === 'acp') {
    return rawModel;
  }
  let formatted = rawModel;
  for (let i = 0; i < catalogs.length; i += 1) {
    const cat = catalogs[i];
    if (cat.provider.trim().toLowerCase() !== provider) {
      continue;
    }
    for (let j = 0; j < cat.models.length; j += 1) {
      const spec = cat.models[j];
      if (spec.id === rawModel) {
        formatted = spec.label;
        break;
      }
    }
  }
  const compact = provider === 'claude_code'
    ? formatted.replace(/^Claude\s+/i, '').trim()
    : formatted.trim();
  return compact.length > 0 && compact.toLowerCase() !== 'default' ? compact : '';
}

export function resolveAgentFallbackLabel(agent: AgentCardDto | null | undefined): string {
  if (agent === null || agent === undefined) {
    return '';
  }
  const provider = agent.provider.trim().toLowerCase();
  if (provider === 'acp') {
    const directAcp = (agent.acp_agent_id ?? '').trim();
    if (directAcp.length > 0) {
      return directAcp;
    }
    if (agent.labels !== undefined && agent.labels !== null) {
      const labelAcp = (agent.labels['orbit.acp_agent_id'] ?? agent.labels['acp_agent_id'] ?? '').trim();
      if (labelAcp.length > 0) {
        return labelAcp;
      }
    }
    return formatProviderLabel('acp');
  }
  if (agent.provider.trim().length > 0) {
    return formatProviderLabel(agent.provider);
  }
  return '';
}

export function resolveAgentEffortDisplay(agent: AgentCardDto | null | undefined): string {
  if (agent === null || agent === undefined) {
    return '';
  }
  return formatEffortLabel(agent.effort);
}

/**
 * Resolves the target host ID for a Stage Crew subagent entry by consuming `sub.host_id`
 * projected by `crates/orbit-client-core/src/stage.rs:406` before falling back to the active host.
 */
export function resolveCrewSubagentHostId(
  sub: SessionSubagentEntryDto,
  fallbackHostId: string
): string {
  const projected = (sub.host_id ?? '').trim();
  return projected.length > 0 ? projected : fallbackHostId.trim();
}

/**
 * Resolves the authoritative tree depth for a Stage Crew subagent entry:
 * consumes core-projected `sub.depth` first (supporting remote subagents at `depth >= 4`),
 * and falls back to walking `parent_agent_id` / `parent_host_id` links across cached hosts.
 */
export function resolveCrewSubagentDepth(
  sub: SessionSubagentEntryDto,
  rootAgentId: string,
  fallbackHostId: string,
  findAgentOnHost: (hostId: string, agentId: string) => AgentCardDto | null
): number {
  if (typeof sub.depth === 'number' && Number.isFinite(sub.depth) && sub.depth > 0) {
    return Math.floor(sub.depth);
  }
  const childId = (sub.child_agent_id ?? '').trim();
  if (childId.length === 0) {
    return 1;
  }
  const startHost = resolveCrewSubagentHostId(sub, fallbackHostId);
  let curr = findAgentOnHost(startHost, childId);
  if (curr === null) {
    return 1;
  }
  let currHost = startHost;
  let depth = 1;
  const visited = new Set<string>([`${startHost}::${childId}`]);
  while (curr !== null && depth < 6) {
    const parentId = (curr.parent_agent_id ?? '').trim();
    if (parentId.length === 0 || parentId === rootAgentId) {
      break;
    }
    const nextHost = (curr.parent_host_id ?? '').trim().length > 0
      ? (curr.parent_host_id ?? '').trim()
      : currHost;
    const visitKey = `${nextHost}::${parentId}`;
    if (visited.has(visitKey)) {
      break;
    }
    visited.add(visitKey);
    const next = findAgentOnHost(nextHost, parentId);
    if (next === null) {
      break;
    }
    depth += 1;
    curr = next;
    currHost = nextHost;
  }
  return depth;
}

/**
 * Maps a Stage Crew `SessionSubagentEntryDto` (and optional cached live `AgentCardDto`) to a
 * canonical list status. Matches Desktop `apps/desktop/src/lib/subagents.ts:229`:
 * - Explicit wire `'denied'` ALWAYS maps to `'failed'` and NEVER compares parent tool `last_seq`
 *   against the child session's independent `latest_seq` stream (preventing stale cached child
 *   `running` or `idle` cards from masking a denied dispatch).
 * - `'failed'` and `'denied'` map to `'failed'` (never `'completed'`).
 */
export function mapCrewSubagentStatus(
  sub: SessionSubagentEntryDto,
  live: AgentCardDto | null
): string {
  const wireLower = (sub.status ?? '').trim().toLowerCase();
  if (wireLower === 'denied') {
    return 'failed';
  }
  if (live !== null) {
    return mapAgentCardToListStatus(live);
  }
  if (wireLower === 'running' || wireLower === 'in_progress') {
    return 'running';
  }
  if (wireLower === 'awaiting_permission') {
    return 'awaiting_permission';
  }
  if (wireLower === 'failed' || wireLower === 'error') {
    return 'failed';
  }
  if (wireLower === 'canceled' || wireLower === 'cancelled' || wireLower === 'stopped') {
    return 'canceled';
  }
  return 'completed';
}
