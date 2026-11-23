import type {
  AgentCardView,
  HostCardView,
  HostDetailView,
  ProviderKind,
  ProviderSubagentDescriptor,
  SessionSubagentEntryView,
  TimelineItemView,
} from "@/lib/orbit-ipc";

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
  canceled: 3,
  completed: 4,
};

function mapManagedStatus(agent: AgentCardView, fallbackStatus?: string): ResolvedSubagentStatus {
  if (agent.is_awaiting_permission) return "awaiting_permission";
  if (agent.is_running || agent.has_background_tasks) return "running";
  const lowerLabel = agent.status_label.toLowerCase();
  if (
    lowerLabel.includes("cancel") ||
    lowerLabel.includes("interrupt") ||
    lowerLabel.includes("stopped")
  ) {
    return "canceled";
  }
  if (
    fallbackStatus === "failed" ||
    fallbackStatus === "denied" ||
    lowerLabel.includes("error") ||
    lowerLabel.includes("fail")
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

export interface ResolvedToolCallSubagent {
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
      x.title !== y.title ||
      x.status !== y.status ||
      x.childAgentId !== y.childAgentId ||
      x.providerSubagentId !== y.providerSubagentId ||
      x.childHostId !== y.childHostId ||
      x.promptPreview !== y.promptPreview
    ) {
      return false;
    }
  }
  return true;
}

export function classifyGenericSubagentTool(toolName: string): "dispatch" | "message" | null {
  const stripped = toolName.trim().replace(/^run\s+/i, "");
  if (!stripped) return null;
  const firstToken = (stripped.split(/\s+/)[0] ?? "").toLowerCase();
  if (!firstToken) return null;
  const leaf = firstToken.split(/[:/.]|__/).pop() ?? firstToken;
  if (
    leaf === "invoke_subagent" ||
    leaf === "spawn_subagent" ||
    leaf === "create_agent" ||
    leaf === "spawn_workspace_peer" ||
    leaf === "mcp_orbit_create_agent" ||
    leaf === "mcp_orbit_spawn_workspace_peer"
  ) {
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

/**
 * Resolves a subagent dispatch (`invoke_subagent`, `create_agent`, `spawn_workspace_peer`, or
 * `detail.kind === "subagent"`) or follow-up messaging (`send_message`, `send_agent_prompt`,
 * `message_peer`) tool call against the session's unified `ResolvedSubagentItem` list.
 */
export function resolveToolCallSubagent(
  item: Extract<TimelineItemView, { item_type: "tool_call" }>,
  subagents: readonly ResolvedSubagentItem[],
  fallbackHostId?: string,
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

  const matchByTitle = (title: string | null | undefined): ResolvedSubagentItem | undefined => {
    const clean = title?.trim();
    if (!clean || clean.toLowerCase() === "subagent") return undefined;
    const lower = clean.toLowerCase();
    return subagents.find((sa) => sa.title.trim().toLowerCase() === lower);
  };

  if (item.detail.kind === "subagent") {
    const isLegacyPlaceholder =
      !item.detail.child_agent_id &&
      !item.detail.prompt_preview.trim() &&
      item.detail.role.trim().toLowerCase() === "subagent";
    if (isLegacyPlaceholder) return null;

    const matched =
      matchById(item.call_id) ??
      matchById(item.detail.child_agent_id) ??
      matchByTitle(item.detail.role);
    if (matched) {
      return { subagent: matched, action: "dispatch" };
    }
    const role = item.detail.role.trim() || "Subagent";
    return {
      subagent: {
        key: `tool:${item.call_id}`,
        callId: item.call_id,
        title: role,
        promptPreview: item.detail.prompt_preview || "",
        status:
          item.status === "running"
            ? "running"
            : item.status === "failed" || item.status === "denied"
              ? "failed"
              : "completed",
        isManaged: false,
        childHostId: fallbackHostId ?? "",
        remoteHostName: null,
        childAgentId: item.detail.child_agent_id,
        childProvider: null,
        providerSubagentId: null,
        sortOrder: item.seq,
      },
      action: "dispatch",
    };
  }

  if (item.detail.kind === "generic") {
    const action = classifyGenericSubagentTool(item.detail.tool_name);
    if (!action) return null;
    const args = tryParseJsonObject(item.detail.arguments_json);
    const targetId = readStringField(args, [
      "Recipient",
      "recipient",
      "agent_id",
      "conversation_id",
      "conversationId",
      "child_agent_id",
    ]);
    const targetRole = readStringField(args, ["Role", "role", "title", "handle"]);
    const matched =
      (action === "dispatch" ? matchById(item.call_id) : undefined) ??
      matchById(targetId) ??
      matchByTitle(targetRole);
    if (matched) {
      return { subagent: matched, action };
    }
  }

  return null;
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
