import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertCircle,
  Archive,
  ArrowUpRight,
  CircleCheck,
  MoreHorizontal,
  Unlink,
} from "lucide-react";
import { ConversationViewport } from "@/components/conversation/conversation-viewport";
import { TurnStageSection } from "@/components/stage/stream-stage";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/overlays";
import { type StageTurnView, orbitIpc } from "@/lib/orbit-ipc";
import { type SidePanelTab, orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const NOOP_HOLD_VIEWPORT = (_el?: HTMLElement | null) => {};
const NOOP_THINKING_DURATION = () => null;
const NOOP_REWIND = () => {};
const NOOP_PREVIEW_IMAGE = () => {};

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
  const status = childAgent
    ? childAgent.is_awaiting_permission
      ? "awaiting_permission"
      : childAgent.is_running
        ? "running"
        : childAgent.status_label.toLowerCase().includes("error")
          ? "failed"
          : "completed"
    : providerSubagent
      ? providerSubagent.status === "running"
        ? "running"
        : providerSubagent.status === "completed"
          ? "completed"
          : "failed"
      : (parentSubagent?.status ?? "completed");

  const resolvedPrompt = tab.subagentPrompt || providerSubagent?.subtitle || "";
  const [providerTurns, setProviderTurns] = useState<StageTurnView[]>([]);
  const lastFingerprintRef = useRef<string>("");

  // Ensure daemon hydrates & subscribes the provider subagent timeline on open or reconnect
  useEffect(() => {
    if (!parentHostId || !parentAgentId || !tab.providerSubagentId) return;
    void orbitIpc.openProviderSubagentTimeline(parentHostId, parentAgentId, tab.providerSubagentId);
  }, [parentHostId, parentAgentId, tab.providerSubagentId]);

  // Project provider subagent events through the exact same Rust stage pipeline reactively
  // whenever `parentHostRevision` ticks (Catchup / Live events bump host revision immediately).
  useEffect(() => {
    if (!parentHostId || !parentAgentId || !tab.providerSubagentId) {
      setProviderTurns([]);
      lastFingerprintRef.current = "";
      return;
    }
    const subId = tab.providerSubagentId;
    let cancelled = false;
    const syncTurns = async () => {
      try {
        const turns = await orbitIpc.getProviderSubagentTurns(
          parentHostId,
          parentAgentId,
          subId,
          resolvedPrompt,
        );
        if (cancelled) return;
        const lastTurn = turns[turns.length - 1];
        const fp = `${turns.length}:${lastTurn?.key ?? ""}:${lastTurn?.live_segments.length ?? 0}:${lastTurn?.folded_steps.length ?? 0}:${lastTurn?.work_counts.total_steps ?? 0}:${Boolean(lastTurn?.terminal_reply)}:${lastTurn?.completed_at_ms ?? 0}`;
        if (fp !== lastFingerprintRef.current || turns.length !== providerTurns.length) {
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

  const contentVersion = `${childStage?.revision ?? 0}:${providerTurns.length}:${parentHostRevision}:${displayTurns[displayTurns.length - 1]?.live_segments.length ?? 0}`;
  const effectiveAgentId = tab.childAgentId ?? tab.providerSubagentId ?? parentAgentId ?? "";
  const effectiveHostId = isManaged ? (hostId ?? "") : (parentHostId ?? "");

  const canArchiveNative = Boolean(!isManaged && parentHostId && parentAgentId);
  const resolvedTitle = tab.subagentRole || tab.title || "Subagent";
  const viewportResetKey = `${tab.id}:${tab.childAgentId ?? tab.providerSubagentId ?? tab.subagentCallId ?? ""}`;

  // Keep the header minimal: tab title already names the subagent, so only render a restrained
  // context strip when the user can manage the thread or when active/error status needs signaling.
  const showContextBar = Boolean(
    isManaged ||
      canArchiveNative ||
      status === "running" ||
      status === "failed" ||
      status === "denied" ||
      remoteHostName,
  );

  return (
    <div className="orbit-stage-enter bg-canvas flex min-h-0 flex-1 flex-col overflow-hidden">
      {showContextBar && (
        <div className="flex h-[32px] shrink-0 items-center justify-between gap-2 border-b border-black/[0.055] px-3.5 dark:border-white/[0.06]">
          <div className="flex min-w-0 items-center gap-2 text-[12px]">
            {status === "awaiting_permission" ? (
              <span className="inline-flex items-center gap-1.5 font-medium text-amber-600/90 dark:text-amber-400/90">
                <span className="size-[6px] rounded-full bg-amber-500 shadow-[0_0_0_2px_rgba(245,158,11,0.18)]" />
                <span>Approval required</span>
              </span>
            ) : status === "running" ? (
              <span className="text-muted-foreground inline-flex items-center gap-1.5">
                <OrbitSpinner size={12.5} className="text-foreground/70" />
                <span>Running</span>
              </span>
            ) : status === "failed" || status === "denied" ? (
              <span className="inline-flex items-center gap-1.5 text-rose-600/90 dark:text-rose-400/90">
                <AlertCircle className="size-[13px]" />
                <span className="capitalize">{status}</span>
              </span>
            ) : (
              <span className="text-muted-foreground inline-flex items-center gap-1.5">
                <CircleCheck className="size-[13px] text-emerald-600/85 dark:text-emerald-400/85" />
                <span>Completed</span>
              </span>
            )}

            {remoteHostName && (
              <span className="text-muted-foreground/65 truncate text-[11.5px]">
                · {remoteHostName}
              </span>
            )}
          </div>

          {(isManaged && hostId && tab.childAgentId) || canArchiveNative ? (
            <div className="flex shrink-0 items-center gap-0.5">
              {isManaged && hostId && tab.childAgentId && (
                <button
                  type="button"
                  onClick={handleOpenThread}
                  className={cn(
                    "inline-flex h-[24px] shrink-0 cursor-pointer items-center gap-1 rounded-[6px] px-2 text-[11.5px] font-medium transition-colors",
                    status === "awaiting_permission"
                      ? "text-amber-700 hover:bg-amber-500/12 dark:text-amber-300"
                      : "text-muted-foreground hover:text-foreground hover:bg-accent",
                  )}
                >
                  <span>{status === "awaiting_permission" ? "Review" : "Open thread"}</span>
                  <ArrowUpRight className="size-[12px]" />
                </button>
              )}
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <button
                    type="button"
                    aria-label="Subagent actions"
                    className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex size-[24px] shrink-0 cursor-pointer items-center justify-center rounded-[6px] transition-colors"
                  >
                    <MoreHorizontal className="size-[13.5px]" />
                  </button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end" side="bottom" sideOffset={4} className="min-w-[164px]">
                  {isManaged && hostId && tab.childAgentId && (
                    <DropdownMenuItem
                      onSelect={() => {
                        void orbitActions.detachAgent(hostId, tab.childAgentId!);
                      }}
                    >
                      <span className="flex items-center gap-2">
                        <Unlink className="text-muted-foreground size-[13.5px]" />
                        <span>Detach to thread</span>
                      </span>
                    </DropdownMenuItem>
                  )}
                  <DropdownMenuItem
                    onSelect={() => {
                      if (isManaged && hostId && tab.childAgentId) {
                        orbitActions.archiveThread(hostId, tab.childAgentId);
                      } else if (parentHostId && parentAgentId) {
                        orbitActions.archiveSubagent(parentHostId, parentAgentId, {
                          id:
                            tab.subagentCallId ||
                            tab.providerSubagentId ||
                            tab.childAgentId ||
                            tab.id,
                          providerSubagentId: tab.providerSubagentId ?? null,
                          childAgentId: tab.childAgentId ?? null,
                          role: resolvedTitle,
                        });
                      }
                      orbitActions.closeSidePanelTab(tab.id);
                    }}
                  >
                    <span className="flex items-center gap-2">
                      <Archive className="text-muted-foreground size-[13.5px]" />
                      <span>Archive</span>
                    </span>
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
          ) : null}
        </div>
      )}

      {/* Main Conversation Stream Parity (identical TurnStageSection, spacing, and typography; no Composer) */}
      <ConversationViewport
        hostId={effectiveHostId}
        cwd={cwd}
        resetKey={viewportResetKey}
        contentVersion={contentVersion}
        viewportClassName="px-6 pt-6 pb-11"
      >
        <div className="orbit-stage-enter mx-auto w-full min-w-0 max-w-[768px]">
          {displayTurns.length > 0 ? (
            <div className="space-y-[32px]">
              {displayTurns.map((turn, idx) => {
                const isLastTurn = idx === displayTurns.length - 1;
                const isTurnActive =
                  isLastTurn && (status === "running" || turn.has_active_item);
                const liveLabel = isLastTurn
                  ? isManaged
                    ? (childAgent?.status_label ?? null)
                    : status === "running"
                      ? "Working..."
                      : null
                  : null;
                return (
                  <TurnStageSection
                    key={turn.key}
                    turn={turn}
                    isTurnActive={isTurnActive}
                    agentStatusLabel={liveLabel}
                    isNetworkOffline={isNetworkOffline}
                    autoFoldCompleted={coalesceExploration}
                    hostId={effectiveHostId}
                    cwd={cwd}
                    agentId={effectiveAgentId}
                    thinkingSignature=""
                    getThinkingDurationMs={NOOP_THINKING_DURATION}
                    holdViewport={NOOP_HOLD_VIEWPORT}
                    onRequestRewind={NOOP_REWIND}
                    onPreviewImage={NOOP_PREVIEW_IMAGE}
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
                  : "Subagent outputs were folded into the main conversation."}
              </span>
            </div>
          )}
        </div>
      </ConversationViewport>
    </div>
  );
});
