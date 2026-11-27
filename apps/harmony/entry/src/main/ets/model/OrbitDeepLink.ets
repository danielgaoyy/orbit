import { HostDetailDto } from './OrbitTypes';

export type DeepLinkAgentValidation = 'selected' | 'pending' | 'not_found';
export type DeepLinkPostNavigationAction = 'none' | 'permission_required' | 'automations' | 'agent_error';

/**
 * Resolves the post-navigation side effect for a deep link.
 * Approval-specific (`permission_required`) and error-specific (`agent_error`) Stage reloads and
 * status toasts require a resolved non-empty `agentId`; host-only deep links only switch hosts.
 */
export function resolveDeepLinkPostNavigationAction(
  eventKind: string,
  agentId: string
): DeepLinkPostNavigationAction {
  const cleanEvent = eventKind.trim();
  const hasAgent = agentId.trim().length > 0;
  if (cleanEvent === 'permission_required') {
    return hasAgent ? 'permission_required' : 'none';
  }
  if (cleanEvent === 'goal_finished' || cleanEvent === 'timer_fired') {
    return 'automations';
  }
  if (cleanEvent === 'agent_error') {
    return hasAgent ? 'agent_error' : 'none';
  }
  return 'none';
}

export interface OrbitParsedOpenDeepLink {
  readonly hostId: string;
  readonly agentId: string;
  readonly eventKind: string;
  readonly requestId: string;
  readonly targetSeq: number;
}

/**
 * Tri-state deep-link session validation against cached `HostDetailDto`:
 * - `'selected'`: target agent exists on the host (safe to open & mark read).
 * - `'pending'`: host detail not yet loaded or host offline/connecting (defer until hydrated).
 * - `'not_found'`: host is online and hydrated, yet agent does not exist (abort side effects).
 */
export function validateDeepLinkAgentInDetail(
  detail: HostDetailDto | null,
  hostId: string,
  agentId: string
): DeepLinkAgentValidation {
  if (agentId.length === 0) {
    return 'selected';
  }
  if (detail === null || detail.host.host_id !== hostId) {
    return 'pending';
  }
  for (let i = 0; i < detail.agents.length; i += 1) {
    if (detail.agents[i].agent_id === agentId) {
      return 'selected';
    }
  }
  const conn = detail.host.connection_state;
  const hostOnline = conn === 'online_direct' || conn === 'online_relay';
  return hostOnline ? 'not_found' : 'pending';
}

function safeDecodeUriComponent(raw: string): string {
  try {
    return decodeURIComponent(raw);
  } catch (_err) {
    return raw;
  }
}

/**
 * Parses `orbit://open?host=<enc>&agent=<enc>` (and optional `event_kind`, `request_id`, `seq`).
 * Returns `null` for non-open URIs or URIs missing a non-empty `host`.
 */
export function parseOrbitOpenDeepLink(
  uri: string,
  fallbackEventKind: string = '',
  fallbackRequestId: string = '',
  fallbackSeq: number = 0
): OrbitParsedOpenDeepLink | null {
  const trimmed = uri.trim();
  if (trimmed.length === 0) {
    return null;
  }
  const hashIdx = trimmed.indexOf('#');
  const clean = hashIdx >= 0 ? trimmed.substring(0, hashIdx).trim() : trimmed;
  const lower = clean.toLowerCase();
  const isOpenRoute =
    lower === 'orbit://open' ||
    lower.startsWith('orbit://open?') ||
    lower.startsWith('orbit://open/');
  if (!isOpenRoute) {
    return null;
  }

  let hostId = '';
  let agentId = '';
  let eventKind = fallbackEventKind.trim();
  let requestId = fallbackRequestId.trim();
  let targetSeq = Number.isFinite(fallbackSeq) && fallbackSeq > 0 ? Math.floor(fallbackSeq) : 0;

  const qIdx = clean.indexOf('?');
  if (qIdx >= 0 && qIdx + 1 < clean.length) {
    const queryStr = clean.substring(qIdx + 1);
    const parts = queryStr.split('&');
    for (let i = 0; i < parts.length; i += 1) {
      const seg = parts[i];
      if (seg.length === 0) {
        continue;
      }
      const eq = seg.indexOf('=');
      if (eq <= 0) {
        continue;
      }
      const key = seg.substring(0, eq).trim().toLowerCase();
      const rawVal = seg.substring(eq + 1).trim();
      const val = safeDecodeUriComponent(rawVal).trim();
      if (key === 'host' || key === 'host_id') {
        hostId = val;
      } else if (key === 'agent' || key === 'agent_id') {
        agentId = val;
      } else if (key === 'event_kind' || key === 'event') {
        eventKind = val;
      } else if (key === 'request_id' || key === 'req') {
        requestId = val;
      } else if (key === 'seq') {
        const parsedSeq = Number.parseInt(val, 10);
        if (Number.isFinite(parsedSeq) && parsedSeq > 0) {
          targetSeq = parsedSeq;
        }
      }
    }
  }

  if (hostId.length === 0) {
    return null;
  }
  return {
    hostId: hostId,
    agentId: agentId,
    eventKind: eventKind,
    requestId: requestId,
    targetSeq: targetSeq
  };
}
