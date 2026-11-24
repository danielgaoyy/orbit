import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ArrowUpRight } from "lucide-react";
import {
  ConversationViewport,
  useViewportAnchorHold,
} from "@/components/conversation/conversation-viewport";
import {
  PromptImageLightbox,
  type PromptImagePreviewTarget,
  TurnStageSection,
} from "@/components/stage/stream-stage";
import { useDelegationDispatches } from "@/components/stage/use-delegation-dispatches";
import { selectSessionSubagents } from "@/state/selectors/session-selectors";
import { ContextBanner } from "@/components/ui/context-banner";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { SubagentAvatar } from "@/components/ui/subagent-avatar";
import {
  type SessionSubagentEntryView,
  type StageTurnView,
  isAgentForegroundRunning,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { computeSubagentTurnsFingerprint } from "@/lib/stage-cache";
import {
  type ResolvedSubagentItem,
  buildSubagentNameMatcher,
} from "@/lib/subagents";
import { type SidePanelTab, orbitActions, useOrbitSelector } from "@/state/orbit-store";

const NOOP_REWIND = () => {};
const EMPTY_STAGE_SUBAGENTS: readonly SessionSubagentEntryView[] = [];

interface ThinkingTimingRecord {
  startedAtMs: number;
  completedAtMs: number | null;
  durationMs: number | null;
}

function buildSubagentTurnThinkingSignature(
  turn: StageTurnView,
  agentId: string,
  thinkingTimings: Record<string, ThinkingTimingRecord>,
): string {
  let sig = "";
  for (const seg of turn.live_segments) {
    if (seg.kind !== "step_group") continue;
    for (const it of seg.items) {
      if (it.item_type !== "assistant_thinking") continue;
      const key = `${agentId}::think::${it.item_id || it.seq}`;
      const dur = thinkingTimings[key]?.durationMs;
      if (dur !== null && dur !== undefined) {
        sig += `${it.seq}:${dur};`;
      }
    }
  }
  return sig;
}

function buildFallbackPromptTurn(callId: string, text: string): StageTurnView {
  return {
    key: `subagent-fallback-${callId}`,
    turn_index: 0,
    anchor_seq: 0,
    user_prompt: {
      item_type: "user_prompt",
      seq: 0,
      message_id: `subagent-prompt-${callId}`,
      text,
      badges: [],
      blocks: [
        {
          kind: "paragraph",
          spans: [
            {
              text,
              bold: false,
              italic: false,
              strikethrough: false,
              inline_code: false,
              link_url: null,
            },
          ],
        },
      ],
      timestamp_ms: null,
    },
    agent_message: null,
    is_compaction_command: false,
    folded_steps: [],
    work_counts: {
      edited_file_count: 0,
      created_file_count: 0,
      command_count: 0,
      read_file_count: 0,
      search_count: 0,
      subagent_count: 0,
      mcp_server_count: 0,
      other_tool_count: 0,
      thought_count: 0,
      narration_count: 0,
      action_steps: 0,
      total_steps: 0,
      failed_count: 0,
      denied_count: 0,
      unrecovered_failure_count: 0,
      additions: 0,
      deletions: 0,
    },
    work_summary_label: "",
    work_summary_live_label: "",
    work_summary_live_detail: null,
    work_summary_includes_failure: false,
    thought_duration_ms: null,
    work_duration_ms: null,
    turn_duration_ms: null,
    terminal_reply: null,
    last_assistant_reply: null,
    notices: [],
    compactions: [],
    live_segments: [],
    turn_diffs: [],
    has_error: false,
    has_unrecovered_error: false,
    has_active_item: false,
    started_at_ms: null,
    completed_at_ms: null,
  };
}

export const SubagentTab = memo(function SubagentTab({ tab }: { tab: SidePanelTab }) {
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const hostId = useOrbitSelector((s) => tab.hostId ?? s.selectedHostId);
  const parentHostId = selectedHostId ?? hostId;
  const coalesceExploration = useOrbitSelector((s) => s.coalesceExploration);
  const remoteHostName = useOrbitSelector((s) => {
    if (!tab.hostId || !s.selectedHostId || tab.hostId === s.selectedHostId) return null;
    return (
      s.fleet.hosts.find((h) => h.host_id === tab.hostId)?.display_name ??
      s.hostDetails[tab.hostId]?.host.display_name ??
      tab.hostId
    );
  });
  const cwd = useOrbitSelector((s) => tab.cwd ?? s.selectedWorkspaceCwd);
  const parentAgentId = useOrbitSelector((s) => tab.parentAgentId ?? s.selectedAgentId);
  const parentHostRevision = useOrbitSelector((s) =>
    parentHostId ? (s.hostDetails[parentHostId]?.revision ?? 0) : 0,
  );
  const isNetworkOffline = useOrbitSelector((s) =>
    hostId
      ? s.fleet.hosts.find((h) => h.host_id === hostId)?.connection_state === "disconnected"
      : false,
  );
  const parentHostConnected = useOrbitSelector((s) => {
    if (!parentHostId) return false;
    const conn =
      s.fleet.hosts.find((h) => h.host_id === parentHostId)?.connection_state ??
      s.hostDetails[parentHostId]?.host.connection_state;
    return conn === "online_direct" || conn === "online_relay";
  });
  const managedHostConnected = useOrbitSelector((s) => {
    if (!hostId) return false;
    const conn =
      s.fleet.hosts.find((h) => h.host_id === hostId)?.connection_state ??
      s.hostDetails[hostId]?.host.connection_state;
    return conn === "online_direct" || conn === "online_relay";
  });
  const childAgent = useOrbitSelector((s) =>
    hostId && tab.childAgentId
      ? (s.hostDetails[hostId]?.agents.find((a) => a.agent_id === tab.childAgentId) ?? null)
      : null,
  );
  const childStage = useOrbitSelector((s) =>
    hostId && tab.childAgentId
      ? (s.stages[`${hostId}::${tab.childAgentId}`] ?? null)
      : null,
  );
  const providerSubagent = useOrbitSelector((s) =>
    parentHostId && parentAgentId && tab.providerSubagentId
      ? (s.hostDetails[parentHostId]?.provider_subagents?.[parentAgentId]?.find(
          (ps) => ps.subagent_id === tab.providerSubagentId,
        ) ?? null)
      : null,
  );
  const parentSubagent = useOrbitSelector((s) => {
    if (!parentHostId || !parentAgentId || !tab.subagentCallId) return null;
    const parentStage = s.stages[`${parentHostId}::${parentAgentId}`];
    return parentStage?.insights.subagents.find((sa) => sa.id === tab.subagentCallId) ?? null;
  });

  const isManaged = Boolean(hostId && tab.childAgentId && childAgent);
  const childStatusLabel = childAgent?.status_label.toLowerCase() ?? "";
  const status: "awaiting_permission" | "running" | "completed" | "canceled" | "failed" = childAgent
    ? childAgent.is_awaiting_permission
      ? "awaiting_permission"
      : isAgentForegroundRunning(childAgent)
        ? "running"
        : childStatusLabel.includes("cancel") ||
            childStatusLabel.includes("interrupt") ||
            childStatusLabel.includes("stopped")
          ? "canceled"
          : childStatusLabel.includes("error") || childStatusLabel.includes("fail")
            ? "failed"
            : "completed"
    : providerSubagent
      ? providerSubagent.status === "running"
        ? "running"
        : providerSubagent.status === "completed"
          ? "completed"
          : providerSubagent.status === "canceled" || providerSubagent.status === "stopped"
            ? "canceled"
            : "failed"
      : parentSubagent?.status === "running"
        ? "running"
        : parentSubagent?.status === "failed" || parentSubagent?.status === "denied"
          ? "failed"
          : "completed";

  const resolvedPrompt = tab.subagentPrompt || providerSubagent?.subtitle || "";
  const [providerTurns, setProviderTurns] = useState<StageTurnView[]>([]);
  const providerTurnsRef = useRef<StageTurnView[]>(providerTurns);
  providerTurnsRef.current = providerTurns;
  const refetchMissGuardRef = useRef<string | null>(null);
  const lastFingerprintRef = useRef<string>("");
  const prevProviderSubKeyRef = useRef<string | null>(null);
  const wasParentHostConnectedRef = useRef<boolean>(parentHostConnected);
  const wasManagedHostConnectedRef = useRef<boolean>(managedHostConnected);
  const isActiveTab = useOrbitSelector((s) => s.sidePanelOpen && s.activeSidePanelTabId === tab.id);
  const wasActiveTabRef = useRef<boolean>(isActiveTab);
  const thinkingTimingsRef = useRef<Record<string, ThinkingTimingRecord>>({});
  const [, setThinkingTick] = useState(0);
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const stickToBottomRef = useRef<boolean>(true);
  const holdViewport = useViewportAnchorHold(viewportRef, stickToBottomRef);
  const [imagePreviewTarget, setImagePreviewTarget] =
    useState<PromptImagePreviewTarget | null>(null);
  const handleCloseImagePreview = useCallback(() => setImagePreviewTarget(null), []);

  const getThinkingDurationMs = useCallback((thinkKey: string): number | null => {
    return thinkingTimingsRef.current[thinkKey]?.durationMs ?? null;
  }, []);

  // Ensure daemon hydrates & subscribes the provider subagent timeline on open, tab activation, or WS reconnect
  useEffect(() => {
    const subKey =
      parentHostId && parentAgentId && tab.providerSubagentId
        ? `${parentHostId}::${parentAgentId}::${tab.providerSubagentId}`
        : null;
    const targetChanged = prevProviderSubKeyRef.current !== subKey;
    const reconnected = parentHostConnected && !wasParentHostConnectedRef.current;
    const reactivated = isActiveTab && !wasActiveTabRef.current;
    prevProviderSubKeyRef.current = subKey;
    wasParentHostConnectedRef.current = parentHostConnected;
    wasActiveTabRef.current = isActiveTab;

    if (!parentHostId || !parentAgentId || !tab.providerSubagentId) return;
    if (targetChanged || reconnected || reactivated) {
      refetchMissGuardRef.current = null;
      void orbitActions.openProviderSubagentTimeline(
        parentHostId,
        parentAgentId,
        tab.providerSubagentId,
      );
    }
  }, [parentHostId, parentAgentId, tab.providerSubagentId, parentHostConnected, isActiveTab]);

  // Also re-subscribe managed child timeline on host WS reconnect while the tab is open
  useEffect(() => {
    const reconnected = managedHostConnected && !wasManagedHostConnectedRef.current;
    wasManagedHostConnectedRef.current = managedHostConnected;
    if (!reconnected || !hostId || !tab.childAgentId) return;
    void orbitIpc.openAgentTimeline(hostId, tab.childAgentId);
    void orbitActions.pullStageDelta(hostId, tab.childAgentId, true);
  }, [managedHostConnected, hostId, tab.childAgentId]);

  // Project provider subagent events through the exact same Rust stage pipeline reactively
  // whenever `parentHostRevision` ticks (Catchup / Live events bump host revision immediately).
  useEffect(() => {
    if (!parentHostId || !parentAgentId || !tab.providerSubagentId) {
      setProviderTurns([]);
      lastFingerprintRef.current = "";
      return;
    }
    const subId = tab.providerSubagentId;
    const subKey = `${parentHostId}::${parentAgentId}::${subId}`;
    let cancelled = false;
    const hasSubstantiveContent = (turns: StageTurnView[]): boolean =>
      turns.some(
        (t) =>
          t.live_segments.length > 0 ||
          t.folded_steps.length > 0 ||
          t.terminal_reply !== null ||
          t.notices.length > 0,
      );
    const syncTurns = async () => {
      try {
        const turns = await orbitIpc.getProviderSubagentTurns(
          parentHostId,
          parentAgentId,
          subId,
          resolvedPrompt,
        );
        if (cancelled) return;
        const incomingHasContent = hasSubstantiveContent(turns);
        if (incomingHasContent) {
          refetchMissGuardRef.current = null;
        } else if (hasSubstantiveContent(providerTurnsRef.current)) {
          // Guard against transient cache miss / eviction wiping an actively viewed transcript:
          // trigger one automatic re-subscribe recovery and preserve existing rendered turns.
          if (refetchMissGuardRef.current !== subKey) {
            refetchMissGuardRef.current = subKey;
            void orbitActions.openProviderSubagentTimeline(parentHostId, parentAgentId, subId);
          }
          return;
        }
        const fp = computeSubagentTurnsFingerprint(turns);
        if (fp !== lastFingerprintRef.current || turns.length !== providerTurnsRef.current.length) {
          lastFingerprintRef.current = fp;
          setProviderTurns(turns);
        }
      } catch {
        // ignore transient IPC error
      }
    };
    void syncTurns();
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    parentHostId,
    parentAgentId,
    tab.providerSubagentId,
    parentHostRevision,
    status,
    providerSubagent?.updated_at,
    resolvedPrompt,
  ]);

  const handleOpenThread = useCallback(() => {
    if (hostId && tab.childAgentId) {
      orbitActions.selectAgent(hostId, tab.childAgentId);
    }
  }, [hostId, tab.childAgentId]);

  const displayTurns = useMemo<StageTurnView[]>(() => {
    if (childStage && childStage.turns.length > 0) {
      return childStage.turns;
    }
    if (providerTurns.length > 0) {
      return providerTurns;
    }
    if (resolvedPrompt.trim()) {
      return [buildFallbackPromptTurn(tab.subagentCallId || tab.id, resolvedPrompt.trim())];
    }
    return [];
  }, [childStage, providerTurns, resolvedPrompt, tab.id, tab.subagentCallId]);

  const displayTurnsFingerprint = useMemo(
    () => computeSubagentTurnsFingerprint(displayTurns),
    [displayTurns],
  );
  const contentVersion = `${childStage?.revision ?? 0}:${parentHostRevision}:${displayTurnsFingerprint}`;
  const effectiveAgentId = tab.childAgentId ?? tab.providerSubagentId ?? parentAgentId ?? "";
  const effectiveHostId = isManaged ? (hostId ?? "") : (parentHostId ?? "");
  const hostDetails = useOrbitSelector((s) => s.hostDetails);
  const fleet = useOrbitSelector((s) => s.fleet);
  const archivedSubagents = useOrbitSelector((s) => s.archivedSubagents);
  const parentStageSubagents = useOrbitSelector((s) =>
    parentHostId && parentAgentId
      ? (s.stages[`${parentHostId}::${parentAgentId}`]?.insights.subagents ?? EMPTY_STAGE_SUBAGENTS)
      : EMPTY_STAGE_SUBAGENTS,
  );

  const childInsights = childStage?.insights.subagents ?? EMPTY_STAGE_SUBAGENTS;
  const children = useMemo(() => selectSessionSubagents(
    { hostDetails, fleet, archivedSubagents }, effectiveHostId, effectiveAgentId, childInsights,
  ).all, [hostDetails, fleet, archivedSubagents, effectiveHostId, effectiveAgentId, childInsights]);
  const siblings = useMemo(() => selectSessionSubagents(
    { hostDetails, fleet, archivedSubagents }, parentHostId, parentAgentId, parentStageSubagents,
  ).all, [hostDetails, fleet, archivedSubagents, parentHostId, parentAgentId, parentStageSubagents]);
  const currentPeer = siblings.find((sa) =>
    (tab.childAgentId && sa.childAgentId === tab.childAgentId) ||
    (tab.providerSubagentId && sa.providerSubagentId === tab.providerSubagentId) ||
    sa.callId === tab.subagentCallId,
  );
  const subagentSessionSubagents = useMemo(() => {
    const peers = new Map(siblings.map((sa) => [sa.key, sa]));
    for (const child of children) peers.set(child.key, child);
    return Array.from(peers.values());
  }, [children, siblings]);
  const dispatches = useDelegationDispatches(displayTurns, children, effectiveHostId, subagentSessionSubagents);

  const subagentNameMatcher = useMemo(
    () => buildSubagentNameMatcher(subagentSessionSubagents),
    [subagentSessionSubagents],
  );

  const handleOpenNestedSubagent = useCallback(
    (sa: ResolvedSubagentItem) => {
      if (sa.status === "awaiting_permission" && sa.isManaged && sa.childAgentId) {
        orbitActions.selectAgent(sa.childHostId, sa.childAgentId);
        return;
      }
      orbitActions.openSubagentTab({
        callId: sa.callId,
        role: sa.title,
        promptPreview: sa.promptPreview,
        childAgentId: sa.childAgentId,
        childHostId: sa.childHostId || undefined,
        providerSubagentId: sa.providerSubagentId,
        parentAgentId: effectiveAgentId || parentAgentId || undefined,
      });
    },
    [effectiveAgentId, parentAgentId],
  );

  const latestTurn = displayTurns[displayTurns.length - 1] ?? null;
  const isLatestTurnActive = Boolean(latestTurn && status === "running");

  // Track live per-thinking-block start/completion durations matching StreamStage
  useEffect(() => {
    if (!effectiveAgentId) return;
    let changed = false;
    const now = Date.now();
    const activeThinkKeys = new Set<string>();

    if (latestTurn) {
      const lastSeg = latestTurn.live_segments[latestTurn.live_segments.length - 1] ?? null;
      const lastSegItem =
        lastSeg?.kind === "step_group" ? (lastSeg.items[lastSeg.items.length - 1] ?? null) : null;

      for (const seg of latestTurn.live_segments) {
        if (seg.kind !== "step_group") continue;
        for (const item of seg.items) {
          if (item.item_type !== "assistant_thinking") continue;

          const thinkKey = `${effectiveAgentId}::think::${item.item_id || item.seq}`;
          const isCurrentlyThinking =
            isLatestTurnActive &&
            lastSegItem?.item_type === "assistant_thinking" &&
            lastSegItem.seq === item.seq;
          const prevThink = thinkingTimingsRef.current[thinkKey];

          if (isCurrentlyThinking) {
            activeThinkKeys.add(thinkKey);
            if (!prevThink) {
              const seededStartMs =
                item.timestamp_ms != null && item.timestamp_ms <= now ? item.timestamp_ms : now;
              thinkingTimingsRef.current[thinkKey] = {
                startedAtMs: seededStartMs,
                completedAtMs: null,
                durationMs: null,
              };
              changed = true;
            }
          } else if (prevThink && prevThink.completedAtMs === null) {
            thinkingTimingsRef.current[thinkKey] = {
              ...prevThink,
              completedAtMs: now,
              durationMs: Math.max(0, now - prevThink.startedAtMs),
            };
            changed = true;
          }
        }
      }
    }

    const prefix = `${effectiveAgentId}::think::`;
    for (const [k, rec] of Object.entries(thinkingTimingsRef.current)) {
      if (k.startsWith(prefix) && rec.completedAtMs === null && !activeThinkKeys.has(k)) {
        thinkingTimingsRef.current[k] = {
          ...rec,
          completedAtMs: now,
          durationMs: Math.max(0, now - rec.startedAtMs),
        };
        changed = true;
      }
    }

    if (changed) {
      setThinkingTick((n) => n + 1);
    }
  }, [effectiveAgentId, latestTurn, isLatestTurnActive]);

  const viewportResetKey = `${tab.id}:${tab.childAgentId ?? tab.providerSubagentId ?? tab.subagentCallId ?? ""}`;

  // Status & Archive already live in Overview; render a minimal 1-line bar ONLY when an approval
  // decision is needed or to quietly identify a cross-host subagent. Zero header bar otherwise.
  const needsApproval = Boolean(isManaged && hostId && tab.childAgentId && status === "awaiting_permission");
  const showInlineContextBar = needsApproval || Boolean(remoteHostName);

  return (
    <div className="orbit-stage-enter bg-surface-base flex min-h-0 flex-1 flex-col overflow-hidden">
      {showInlineContextBar && (
        <ContextBanner
          variant="strip"
          tone={needsApproval ? "warning" : "neutral"}
          icon={
            <SubagentAvatar
              character={currentPeer?.character}
              seedKey={tab.childAgentId || tab.providerSubagentId || tab.subagentCallId || tab.title}
              status={status}
              size="xs"
            />
          }
          actions={
            needsApproval ? (
              <button
                type="button"
                onClick={handleOpenThread}
                className="text-signal-warning hover:bg-signal-warning-bg inline-flex h-[22px] shrink-0 cursor-pointer items-center gap-1 rounded-[6px] px-2 text-[11px] font-medium transition-colors"
              >
                <span>Review</span>
                <ArrowUpRight className="size-3" />
              </button>
            ) : undefined
          }
        >
          <span className="inline-flex items-center gap-2">
            {needsApproval && <span className="font-medium">Approval required</span>}
            {remoteHostName && (
              <span className="bg-surface-recessed text-ink-tertiary border-border-subtle inline-flex max-w-[180px] items-center truncate rounded-[6px] border px-1.5 py-0.5 text-[11px] leading-none">
                {remoteHostName}
              </span>
            )}
          </span>
        </ContextBanner>
      )}

      {/* Main Conversation Stream Parity (identical TurnStageSection, spacing, and typography; no Composer) */}
      <ConversationViewport
        hostId={effectiveHostId}
        cwd={cwd}
        resetKey={viewportResetKey}
        contentVersion={contentVersion}
        viewportRef={viewportRef}
        stickToBottomRef={stickToBottomRef}
        viewportClassName="px-6 pt-6 pb-11"
      >
        <div className="orbit-stage-enter mx-auto w-full min-w-0 max-w-[740px]">
          {displayTurns.length > 0 ? (
            <div className="space-y-[32px]">
              {displayTurns.map((turn, idx) => {
                const isLastTurn = idx === displayTurns.length - 1;
                const isTurnActive = isLastTurn && status === "running";
                const liveLabel = isLastTurn
                  ? isManaged
                    ? (childAgent?.status_label ?? null)
                    : status === "running"
                      ? "Working..."
                      : null
                  : null;
                const thinkingSignature = buildSubagentTurnThinkingSignature(
                  turn,
                  effectiveAgentId,
                  thinkingTimingsRef.current,
                );
                return (
                  <TurnStageSection
                    key={turn.key}
                    turn={turn}
                    dispatches={dispatches}
                    isTurnActive={isTurnActive}
                    agentStatusLabel={liveLabel}
                    isNetworkOffline={isNetworkOffline}
                    autoFoldCompleted={coalesceExploration}
                    hostId={effectiveHostId}
                    cwd={cwd}
                    agentId={effectiveAgentId}
                    thinkingSignature={thinkingSignature}
                    getThinkingDurationMs={getThinkingDurationMs}
                    holdViewport={holdViewport}
                    onRequestRewind={NOOP_REWIND}
                    onPreviewImage={setImagePreviewTarget}
                    sessionSubagents={subagentSessionSubagents}
                    sessionSubagentMatcher={subagentNameMatcher}
                    onOpenSubagent={handleOpenNestedSubagent}
                    parentMessageLabel="Sent to parent"
                  />
                );
              })}
            </div>
          ) : (
            <div className="text-muted-foreground flex flex-col items-center justify-center gap-2.5 py-20 text-center text-[13px]">
              {status === "running" && <OrbitSpinner size={16} className="text-foreground/65" />}
              <span>
                {status === "running"
                  ? "Running in an isolated context…"
                  : status === "canceled"
                    ? "Subagent run was canceled."
                    : "Subagent outputs were folded into the main conversation."}
              </span>
            </div>
          )}
        </div>
      </ConversationViewport>

      <PromptImageLightbox
        target={imagePreviewTarget}
        onClose={handleCloseImagePreview}
      />
    </div>
  );
});
