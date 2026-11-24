import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowUp,
  ArrowUpRight,
  FileText,
  GitBranch,
  Square,
} from "lucide-react";
import {
  ConversationViewport,
  useViewportAnchorHold,
} from "@/components/conversation/conversation-viewport";
import { mergeAgentPendingPermissions } from "@/components/side-panel/subagent-tab";
import { DecisionDock } from "@/components/stage/stage-approval-dock";
import { PlanReviewCard } from "@/components/stage/plan-review-card";
import {
  PromptImageLightbox,
  type PromptImagePreviewTarget,
  TurnStageSection,
} from "@/components/stage/stream-stage";
import {
  createStageSessionSelector,
  getTurnFooter,
  getTurnTopGapPx,
} from "@/components/stage/stage-helpers";
import { openSubagent } from "@/components/stage/subagent-crew-dock";
import { useDelegationDispatches } from "@/components/stage/use-delegation-dispatches";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { ProviderIcon } from "@/components/ui/provider-icons";
import { useI18n } from "@/lib/i18n";
import { useImeCompositionGuard } from "@/lib/ime";
import {
  type StageTurnView,
  isAgentForegroundRunning,
  orbitIpc,
  semanticAgentTabLabel,
} from "@/lib/orbit-ipc";
import {
  type ResolvedSubagentItem,
  buildSubagentNameMatcher,
} from "@/lib/subagents";
import {
  type SidePanelTab,
  orbitActions,
  threadScopeKey,
  useOrbitSelector,
} from "@/state/orbit-store";

const NOOP_REWIND = () => {};
const NOOP_THINKING_DURATION = () => null;
const EMPTY_TURNS: StageTurnView[] = [];

export function recoverReplyDraftAfterSendError(
  currentDraft: string,
  failedTrimmedDraft: string,
): string {
  return currentDraft.trim() ? currentDraft : failedTrimmedDraft;
}

export const PeerSplitTab = memo(function PeerSplitTab({ tab }: { tab: SidePanelTab }) {
  const { t } = useI18n();
  const hostId = tab.hostId ?? "";
  const peerAgentId = tab.peerAgentId ?? "";
  const tKey = threadScopeKey(hostId, peerAgentId);

  const stage = useOrbitSelector((s) => s.stages[tKey] ?? null);
  const hostDetail = useOrbitSelector((s) => (hostId ? (s.hostDetails[hostId] ?? null) : null));
  const coalesceExploration = useOrbitSelector((s) => s.coalesceExploration);
  const connectionState = useOrbitSelector((s) =>
    hostId
      ? (s.fleet.hosts.find((h) => h.host_id === hostId)?.connection_state ??
        s.hostDetails[hostId]?.host.connection_state)
      : null,
  );
  const isNetworkOffline = connectionState === "disconnected";
  const agent =
    hostDetail?.agents.find((a) => a.agent_id === peerAgentId) ?? stage?.agent ?? null;
  const workspace = agent
    ? (hostDetail?.workspaces.find((w) => w.workspace_id === agent.workspace_id) ?? null)
    : null;
  const cwd = agent?.cwd ?? tab.cwd ?? "";
  const peerForegroundRunning = isAgentForegroundRunning(agent);

  const selectPeerSession = useMemo(
    () => createStageSessionSelector(hostId || null, peerAgentId || null),
    [hostId, peerAgentId],
  );
  const { subagents: peerSessionSubagents, decisions: peerDecisions } =
    useOrbitSelector(selectPeerSession);
  const pendingPlanApproval = peerDecisions.pendingPlanApproval;
  const pendingPermissions = useMemo(
    () =>
      mergeAgentPendingPermissions(
        peerAgentId,
        peerDecisions.parentPermissions,
        stage?.pending_permissions,
        hostDetail?.pending_permissions,
        pendingPlanApproval?.request_id ?? null,
      ),
    [
      peerAgentId,
      peerDecisions.parentPermissions,
      stage?.pending_permissions,
      hostDetail?.pending_permissions,
      pendingPlanApproval?.request_id,
    ],
  );
  const pendingQuestions = peerDecisions.parentQuestions;
  const subagentPermissions = peerDecisions.subagentPermissions;
  const hasPendingDecisions =
    pendingPlanApproval !== null ||
    pendingPermissions.length > 0 ||
    pendingQuestions.length > 0 ||
    subagentPermissions.length > 0;
  const peerNameMatcher = useMemo(
    () => buildSubagentNameMatcher(peerSessionSubagents),
    [peerSessionSubagents],
  );
  const handleOpenNestedSubagent = useCallback(
    (sa: ResolvedSubagentItem) => openSubagent(peerAgentId || undefined, sa),
    [peerAgentId],
  );

  const [reply, setReply] = useState("");
  const [sending, setSending] = useState(false);
  const [hydrating, setHydrating] = useState(false);
  const [imagePreviewTarget, setImagePreviewTarget] =
    useState<PromptImagePreviewTarget | null>(null);
  const handleCloseImagePreview = useCallback(() => setImagePreviewTarget(null), []);
  const replyIme = useImeCompositionGuard();
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const stickToBottomRef = useRef<boolean>(true);
  const holdViewport = useViewportAnchorHold(viewportRef, stickToBottomRef);

  const turns = stage?.turns ?? EMPTY_TURNS;
  const dispatches = useDelegationDispatches(turns, peerSessionSubagents, hostId);

  useEffect(() => {
    if (!hostId || !peerAgentId) return;
    let cancelled = false;
    setHydrating(turns.length === 0);
    // Pull cached snapshot immediately if available, then sync after subscription opens.
    void orbitActions.pullStageDelta(hostId, peerAgentId, true);
    void orbitIpc
      .openAgentTimeline(hostId, peerAgentId)
      .then(() => {
        if (!cancelled) return orbitActions.pullStageDelta(hostId, peerAgentId, false);
      })
      .catch(() => undefined)
      .finally(() => {
        if (!cancelled) setHydrating(false);
      });
    return () => {
      cancelled = true;
    };
    // Hydrate only when switching peer target
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hostId, peerAgentId]);

  const isRunning = isAgentForegroundRunning(agent);
  const lastTurn = turns[turns.length - 1];
  const contentVersion = `${turns.length}:${stage?.latest_seq ?? 0}:${lastTurn?.live_segments.length ?? 0}:${isRunning ? 1 : 0}`;

  const handleSend = async () => {
    const trimmed = reply.trim();
    if (!trimmed || !hostId || !peerAgentId || sending) return;
    setSending(true);
    setReply("");
    stickToBottomRef.current = true;
    try {
      await orbitIpc.sendPrompt(hostId, peerAgentId, trimmed, {
        displayPrompt: trimmed,
        activeTurnBehavior: peerForegroundRunning ? "steer" : "queue",
      });
    } catch (err) {
      setReply((cur) => recoverReplyDraftAfterSendError(cur, trimmed));
      const detail = err instanceof Error ? err.message.trim() : String(err).trim();
      orbitActions.showToast(
        detail ? t("approval.submitFailed", { error: detail }) : t("common.actionFailed"),
      );
    } finally {
      setSending(false);
    }
  };

  const handleLabel = agent ? semanticAgentTabLabel(agent) : tab.title;

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      {/* Split Header */}
      <div className="border-border-subtle flex items-center justify-between gap-2 border-b px-3 py-2">
        <div className="flex min-w-0 items-center gap-2">
          {peerForegroundRunning ? (
            <OrbitSpinner size={13} className="text-ink-secondary shrink-0" />
          ) : agent ? (
            <ProviderIcon
              provider={agent.provider}
              className="size-[14px] shrink-0 opacity-80"
            />
          ) : null}
          <div className="min-w-0">
            <div className="flex items-center gap-1.5">
              <span className="text-ink-primary truncate text-[13px] font-semibold">
                {handleLabel}
              </span>
              {agent?.worktree_branch && (
                <span className="bg-surface-recessed text-ink-tertiary inline-flex items-center gap-0.5 rounded px-1.5 py-0.5 font-mono text-[11px]">
                  <GitBranch className="size-[10px]" />
                  {agent.worktree_branch}
                </span>
              )}
            </div>
            <div className="text-ink-tertiary truncate text-[11px]">
              {agent?.task_summary || agent?.title || peerAgentId}
            </div>
          </div>
        </div>

        <div className="flex shrink-0 items-center gap-1">
          {agent?.workspace_id && (
            <button
              type="button"
              title="Open Workspace Board"
              onClick={() =>
                orbitActions.openWorkspaceBoardTab(
                  hostId,
                  agent.workspace_id,
                  workspace?.title,
                )
              }
              className="text-ink-tertiary hover:text-ink-primary hover:bg-accent inline-flex h-[26px] cursor-pointer items-center gap-1 rounded-[6px] px-1.5 text-[13px] leading-none transition-colors"
            >
              <FileText className="size-3" />
            </button>
          )}
          <button
            type="button"
            onClick={() => orbitActions.selectAgent(hostId, peerAgentId)}
            className="border-border-default text-ink-secondary hover:bg-accent hover:text-ink-primary inline-flex h-[26px] cursor-pointer items-center gap-1 rounded-[6px] border px-2 text-[13px] leading-none transition-colors"
          >
            <span>Focus</span>
            <ArrowUpRight className="size-3" />
          </button>
        </div>
      </div>

      {/* Shared Conversation Viewport + TurnStageSection Parity */}
      <ConversationViewport
        hostId={hostId}
        cwd={cwd}
        resetKey={tKey}
        contentVersion={contentVersion}
        viewportRef={viewportRef}
        stickToBottomRef={stickToBottomRef}
        viewportClassName="px-5 pt-5 pb-[24px]"
      >
        <div className="orbit-stage-enter mx-auto w-full min-w-0 max-w-[740px]">
          {turns.length > 0 ? (
            <div>
              {(() => {
                let previousHasFooter: boolean | undefined;
                return turns.map((turn, idx) => {
                  const isLastTurn = idx === turns.length - 1;
                  const isTurnActive = isLastTurn && isRunning;
                  const topGapPx = getTurnTopGapPx(turn, previousHasFooter);
                  previousHasFooter =
                    getTurnFooter(turn, {
                      isActive: isTurnActive,
                      isLastTurn,
                      canFork: false,
                    }) !== null;
                  return (
                    <div
                      key={turn.key}
                      style={topGapPx > 0 ? { paddingTop: topGapPx } : undefined}
                    >
                      <TurnStageSection
                        turn={turn}
                        dispatches={dispatches}
                        isTurnActive={isTurnActive}
                        isLastTurn={isLastTurn}
                        agentStatusLabel={isTurnActive ? (agent?.status_label ?? null) : null}
                        isNetworkOffline={isNetworkOffline}
                        autoFoldCompleted={coalesceExploration}
                        hostId={hostId}
                        cwd={cwd}
                        agentId={peerAgentId}
                        thinkingSignature=""
                        getThinkingDurationMs={NOOP_THINKING_DURATION}
                        holdViewport={holdViewport}
                        onRequestRewind={NOOP_REWIND}
                        onPreviewImage={setImagePreviewTarget}
                        sessionSubagents={peerSessionSubagents}
                        sessionSubagentMatcher={peerNameMatcher}
                        onOpenSubagent={handleOpenNestedSubagent}
                      />
                    </div>
                  );
                });
              })()}
            </div>
          ) : hydrating ? (
            <div role="status" className="text-ink-muted flex items-center justify-center gap-2 py-8 text-[13px]">
              <OrbitSpinner size={14} />
              {t("common.syncing")}
            </div>
          ) : (
            <div className="text-ink-tertiary py-8 text-center text-[13px]">
              {peerForegroundRunning ? "Peer is working…" : "No turns yet."}
            </div>
          )}
        </div>
      </ConversationViewport>

      {/* Actionable Pending Approval / Clarification Dock */}
      {hasPendingDecisions && hostId && peerAgentId && (
        <div className="bg-surface-base relative z-20 shrink-0 pt-2">
          {pendingPlanApproval && (
            <PlanReviewCard
              hostId={hostId}
              agentId={peerAgentId}
              cwd={cwd}
              plan={pendingPlanApproval}
              compact
            />
          )}
          <DecisionDock
            hostId={hostId}
            agentId={peerAgentId}
            cwd={cwd}
            permissions={pendingPermissions}
            questions={pendingQuestions}
            subagentPermissions={subagentPermissions}
            registerGlobalShortcuts={false}
            compact
          />
        </div>
      )}

      {/* Quick Peer Reply Bar */}
      <div className="border-border-subtle border-t p-2.5">
        <div className="border-border-default bg-surface-raised flex items-center gap-1.5 rounded-xl border px-2.5 py-1.5">
          <input
            value={reply}
            onChange={(e) => setReply(e.target.value)}
            {...replyIme.compositionProps}
            onKeyDown={(e) => {
              if (replyIme.shouldIgnoreKeyDown(e)) return;
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                void handleSend();
              }
            }}
            placeholder={`Message ${handleLabel}…`}
            className="text-ink-primary placeholder:text-ink-faint min-w-0 flex-1 bg-transparent text-[13px] leading-[18px] outline-none"
          />
          {peerForegroundRunning && !reply.trim() ? (
            <button
              type="button"
              title="Stop peer"
              onClick={() => void orbitIpc.interruptAgent(hostId, peerAgentId)}
              className="bg-signal-danger-bg text-signal-danger hover:bg-signal-danger/20 inline-flex size-[24px] cursor-pointer items-center justify-center rounded-full transition-colors"
            >
              <Square className="size-[10px] fill-current" />
            </button>
          ) : (
            <button
              type="button"
              disabled={!reply.trim() || sending}
              onClick={() => void handleSend()}
              className="bg-action-primary-bg text-action-primary-fg hover:bg-action-primary-hover inline-flex size-[24px] cursor-pointer items-center justify-center rounded-full transition-colors disabled:opacity-35"
            >
              <ArrowUp className="size-[13px] stroke-[2.15]" />
            </button>
          )}
        </div>
      </div>

      <PromptImageLightbox
        target={imagePreviewTarget}
        onClose={handleCloseImagePreview}
      />
    </div>
  );
});
