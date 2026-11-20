import { memo, useCallback, useEffect, useRef, useState } from "react";
import { ArrowUpRight, Check, Layers, Loader2, AlertTriangle } from "lucide-react";
import {
  ConversationUserBubble,
  ConversationViewport,
} from "@/components/conversation/conversation-viewport";
import { TurnStageSection } from "@/components/stage/stream-stage";
import { type ProviderSubagentEvent, orbitIpc } from "@/lib/orbit-ipc";
import { type SidePanelTab, orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

function formatProviderSubagentPayload(payload: unknown): string {
  if (!payload || typeof payload !== "object") return String(payload ?? "");
  const obj = payload as Record<string, unknown>;
  if (typeof obj.text === "string" && obj.text.trim()) return obj.text.trim();
  if (typeof obj.message === "string" && obj.message.trim()) return obj.message.trim();
  if (obj.detail && typeof obj.detail === "object") {
    const d = obj.detail as Record<string, unknown>;
    if (typeof d.command === "string") return `$ ${d.command}`;
    if (typeof d.path === "string") return `file: ${d.path}`;
    if (typeof d.tool_name === "string") return `tool: ${d.tool_name}`;
  }
  return JSON.stringify(payload).slice(0, 400);
}

const NOOP_HOLD_VIEWPORT = (_el?: HTMLElement | null) => {};
const NOOP_THINKING_DURATION = () => null;
const NOOP_REWIND = () => {};
const NOOP_PREVIEW_IMAGE = () => {};

export const SubagentTab = memo(function SubagentTab({ tab }: { tab: SidePanelTab }) {
  const hostId = useOrbitSelector((s) => tab.hostId ?? s.selectedHostId);
  const cwd = useOrbitSelector((s) => tab.cwd ?? s.selectedWorkspaceCwd);
  const parentAgentId = useOrbitSelector((s) => tab.parentAgentId ?? s.selectedAgentId);
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
    hostId && parentAgentId && tab.providerSubagentId
      ? (s.hostDetails[hostId]?.provider_subagents?.[parentAgentId]?.find(
          (ps) => ps.subagent_id === tab.providerSubagentId,
        ) ?? null)
      : null,
  );
  const parentSubagent = useOrbitSelector((s) => {
    if (!hostId || !parentAgentId || !tab.subagentCallId) return null;
    const parentStage = s.stages[`${hostId}::${parentAgentId}`];
    return parentStage?.insights.subagents.find((sa) => sa.id === tab.subagentCallId) ?? null;
  });

  const status = childAgent
    ? childAgent.is_running
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

  const [providerEvents, setProviderEvents] = useState<ProviderSubagentEvent[]>([]);
  const lastFingerprintRef = useRef<string>("");

  // Ensure daemon hydrates/subscribes the provider subagent timeline on open or reconnect
  useEffect(() => {
    if (!hostId || !parentAgentId || !tab.providerSubagentId) return;
    void orbitIpc.openProviderSubagentTimeline(hostId, parentAgentId, tab.providerSubagentId);
  }, [hostId, parentAgentId, tab.providerSubagentId]);

  useEffect(() => {
    if (!hostId || !parentAgentId || !tab.providerSubagentId) {
      setProviderEvents([]);
      lastFingerprintRef.current = "";
      return;
    }
    const subId = tab.providerSubagentId;
    let cancelled = false;
    const poll = async () => {
      try {
        const evs = await orbitIpc.getProviderSubagentTimeline(hostId, parentAgentId, subId);
        if (cancelled) return;
        const fp = `${evs.length}:${evs[evs.length - 1]?.seq ?? 0}`;
        if (fp !== lastFingerprintRef.current) {
          lastFingerprintRef.current = fp;
          setProviderEvents(evs);
        }
      } catch {
        // ignore transient polling error
      }
    };
    void poll();
    if (status !== "running") {
      return () => {
        cancelled = true;
      };
    }
    const timer = window.setInterval(() => void poll(), 1500);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [hostId, parentAgentId, tab.providerSubagentId, status]);

  const handleOpenThread = useCallback(() => {
    if (hostId && tab.childAgentId) {
      orbitActions.selectAgent(hostId, tab.childAgentId);
    }
  }, [hostId, tab.childAgentId]);

  const resolvedTitle =
    providerSubagent?.title || tab.subagentRole || childAgent?.title || "Subagent";
  const resolvedPrompt = tab.subagentPrompt || providerSubagent?.subtitle || "";
  const contentVersion = `${childStage?.revision ?? 0}:${providerEvents.length}:${providerEvents[providerEvents.length - 1]?.seq ?? 0}`;
  const hasChildTurns = Boolean(childStage && childStage.turns.length > 0);

  return (
    <div className="orbit-stage-enter flex min-h-0 flex-1 flex-col overflow-hidden">
      {/* Subagent Header Sub-Bar */}
      <div className="border-border flex h-[36px] shrink-0 items-center justify-between gap-2 border-b px-3">
        <div className="flex min-w-0 items-center gap-2">
          <Layers className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0" />
          <span className="text-foreground truncate text-[12.5px] font-medium">
            {resolvedTitle}
          </span>
          <span
            className={cn(
              "inline-flex shrink-0 items-center gap-1 rounded-full px-1.5 py-0.5 text-[10.5px] font-medium",
              status === "running" && "bg-foreground/10 text-foreground",
              status === "completed" && "bg-emerald-500/12 text-emerald-700 dark:text-emerald-400",
              (status === "failed" || status === "denied") &&
                "bg-rose-500/12 text-rose-700 dark:text-rose-400",
            )}
          >
            {status === "running" ? (
              <Loader2 className="h-[10px] w-[10px] animate-spin" />
            ) : status === "completed" ? (
              <Check className="h-[10px] w-[10px]" />
            ) : (
              <AlertTriangle className="h-[10px] w-[10px]" />
            )}
            <span className="capitalize">{status}</span>
          </span>
        </div>

        {hostId && tab.childAgentId && childAgent && (
          <button
            type="button"
            onClick={handleOpenThread}
            className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[24px] shrink-0 cursor-pointer items-center gap-1 rounded-md px-2 text-[11.5px] transition-colors"
          >
            <span>Open thread</span>
            <ArrowUpRight className="h-[12px] w-[12px]" />
          </button>
        )}
      </div>

      {/* Full Main Conversation View (read-only, without Composer) */}
      <ConversationViewport
        hostId={hostId}
        cwd={cwd}
        resetKey={tab.id}
        contentVersion={contentVersion}
        viewportClassName="space-y-6 px-5 py-5"
      >
        {!hasChildTurns && resolvedPrompt && <ConversationUserBubble text={resolvedPrompt} />}

        {hasChildTurns && childStage ? (
          <div className="space-y-8">
            {childStage.turns.map((turn, idx) => {
              const isLastTurn = idx === childStage.turns.length - 1;
              const isTurnActive =
                isLastTurn && (Boolean(childAgent?.is_running) || turn.has_active_item);
              return (
                <TurnStageSection
                  key={turn.key}
                  turn={turn}
                  isTurnActive={isTurnActive}
                  agentStatusLabel={isLastTurn ? (childAgent?.status_label ?? null) : null}
                  isNetworkOffline={isNetworkOffline}
                  autoFoldCompleted
                  hostId={hostId ?? ""}
                  cwd={cwd}
                  agentId={tab.childAgentId ?? ""}
                  thinkingSignature=""
                  getThinkingDurationMs={NOOP_THINKING_DURATION}
                  holdViewport={NOOP_HOLD_VIEWPORT}
                  onRequestRewind={NOOP_REWIND}
                  onPreviewImage={NOOP_PREVIEW_IMAGE}
                />
              );
            })}
          </div>
        ) : tab.providerSubagentId && providerEvents.length > 0 ? (
          <div className="space-y-2.5">
            <div className="text-muted-foreground text-[11px] font-medium tracking-wide uppercase">
              Provider Activity ({providerEvents.length}{" "}
              {providerEvents.length === 1 ? "event" : "events"})
            </div>
            <div className="border-border bg-elevated/50 divide-border/50 selectable-text divide-y rounded-xl border font-mono text-[12px]">
              {providerEvents.map((ev) => (
                <div key={ev.seq} className="px-3 py-2 break-words whitespace-pre-wrap">
                  <span className="text-muted-foreground mr-2">#{ev.seq}</span>
                  <span className="text-foreground/90">
                    {formatProviderSubagentPayload(ev.payload)}
                  </span>
                </div>
              ))}
            </div>
          </div>
        ) : (
          <div className="border-border text-muted-foreground rounded-xl border border-dashed px-4 py-6 text-center text-[12.5px]">
            {status === "running"
              ? "Subagent is running in an isolated context. Final results will return to the main thread."
              : "Subagent execution completed. Summary and outputs are folded into the parent conversation."}
          </div>
        )}
      </ConversationViewport>
    </div>
  );
});
