import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowUp,
  ArrowUpRight,
  FileText,
  GitBranch,
  Square,
  X,
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

export interface AgentStagePaneProps {
  hostId: string;
  agentId: string;
  fallbackTitle?: string;
  fallbackCwd?: string;
  hostBadge?: string | null;
  showBoardButton?: boolean;
  onClose?: () => void;
}

/**
 * Shared self-contained live agent viewport pane (streaming turns + PlanReviewCard +
 * DecisionDock + inline quick reply) reused identically across:
 * - Right Side Panel `PeerSplitTab`
 * - `PersonalAgentControlView` side-by-side live watch lanes
 */
export const AgentStagePane = memo(function AgentStagePane({
  hostId,
  agentId,
  fallbackTitle,
  fallbackCwd,
  hostBadge,
  showBoardButton = true,
  onClose,
}: AgentStagePaneProps) {
  const { t } = useI18n();
  const tKey = threadScopeKey(hostId, agentId);

  const stage = useOrbitSelector((s) => s.stages[tKey] ?? null);
  const hostDetail = useOrbitSelector((s) => (hostId ? (s.hostDetails[hostId] ?? null) : null));
  const customTitle = useOrbitSelector((s) => s.threadCustomTitles[tKey] ?? null);
  const coalesceExploration = useOrbitSelector((s) => s.coalesceExploration);
  const connectionState = useOrbitSelector((s) =>
    hostId
      ? (s.fleet.hosts.find((h) => h.host_id === hostId)?.connection_state ??
        s.hostDetails[hostId]?.host.connection_state)
      : null,
  );
  const isNetworkOffline = connectionState === "disconnected";
  const agent =
    hostDetail?.agents.find((a) => a.agent_id === agentId) ?? stage?.agent ?? null;
  const workspace = agent
    ? (hostDetail?.workspaces.find((w) => w.workspace_id === agent.workspace_id) ?? null)
    : null;
  const cwd = agent?.cwd ?? fallbackCwd ?? "";
  const foregroundRunning = isAgentForegroundRunning(agent);

  const selectSession = useMemo(
    () => createStageSessionSelector(hostId || null, agentId || null),
    [hostId, agentId],
  );
  const { subagents: sessionSubagents, decisions } = useOrbitSelector(selectSession);
  const pendingPlanApproval = decisions.pendingPlanApproval;
  const pendingPermissions = useMemo(
    () =>
      mergeAgentPendingPermissions(
        agentId,
        decisions.parentPermissions,
        stage?.pending_permissions,
        hostDetail?.pending_permissions,
        pendingPlanApproval?.request_id ?? null,
      ),
    [
      agentId,
      decisions.parentPermissions,
      stage?.pending_permissions,
      hostDetail?.pending_permissions,
      pendingPlanApproval?.request_id,
    ],
  );
  const pendingQuestions = decisions.parentQuestions;
  const subagentPermissions = decisions.subagentPermissions;
  const hasPendingDecisions =
    pendingPlanApproval !== null ||
    pendingPermissions.length > 0 ||
    pendingQuestions.length > 0 ||
    subagentPermissions.length > 0;
  const nameMatcher = useMemo(
    () => buildSubagentNameMatcher(sessionSubagents),
    [sessionSubagents],
  );
  const handleOpenNestedSubagent = useCallback(
    (sa: ResolvedSubagentItem) => openSubagent(agentId || undefined, sa),
    [agentId],
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
  const dispatches = useDelegationDispatches(turns, sessionSubagents, hostId);

  useEffect(() => {
    if (!hostId || !agentId) return;
    let cancelled = false;
    setHydrating(turns.length === 0);
    void orbitActions.pullStageDelta(hostId, agentId, true);
    void orbitIpc
      .openAgentTimeline(hostId, agentId)
      .then(() => {
        if (!cancelled) return orbitActions.pullStageDelta(hostId, agentId, false);
      })
      .catch(() => undefined)
      .finally(() => {
        if (!cancelled) setHydrating(false);
      });
    return () => {
      cancelled = true;
    };
    // Hydrate only when switching pane target
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hostId, agentId]);

  const isRunning = isAgentForegroundRunning(agent);
  const lastTurn = turns[turns.length - 1];
  const contentVersion = `${turns.length}:${stage?.latest_seq ?? 0}:${lastTurn?.live_segments.length ?? 0}:${isRunning ? 1 : 0}`;

  const handleSend = async () => {
    const trimmed = reply.trim();
    if (!trimmed || !hostId || !agentId || sending) return;
    setSending(true);
    setReply("");
    stickToBottomRef.current = true;
    try {
      await orbitIpc.sendPrompt(hostId, agentId, trimmed, {
        displayPrompt: trimmed,
        activeTurnBehavior: foregroundRunning ? "steer" : "queue",
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

  const handleLabel = customTitle || (agent ? semanticAgentTabLabel(agent) : fallbackTitle || agentId);

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      {/* Pane Header */}
      <div className="border-border-subtle flex items-center justify-between gap-2 border-b px-3 py-2">
        <div className="flex min-w-0 items-center gap-2">
          {foregroundRunning ? (
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
              {hostBadge && (
                <span className="bg-surface-recessed text-ink-tertiary shrink-0 rounded px-1.5 py-0.5 text-[11px]">
                  {hostBadge}
                </span>
              )}
              {agent?.worktree_branch && (
                <span className="bg-surface-recessed text-ink-tertiary inline-flex max-w-[120px] items-center gap-0.5 truncate rounded px-1.5 py-0.5 font-mono text-[11px]">
                  <GitBranch className="size-[10px] shrink-0" />
                  <span className="truncate">{agent.worktree_branch}</span>
                </span>
              )}
            </div>
            <div className="text-ink-tertiary truncate text-[11px]">
              {agent?.task_summary || agent?.status_label || agent?.title || agentId}
            </div>
          </div>
        </div>

        <div className="flex shrink-0 items-center gap-1">
          {showBoardButton && agent?.workspace_id && (
            <button
              type="button"
              title={t("overview.openBoard")}
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
            onClick={() => orbitActions.selectAgent(hostId, agentId)}
            className="border-border-default text-ink-secondary hover:bg-accent hover:text-ink-primary inline-flex h-[26px] cursor-pointer items-center gap-1 rounded-[6px] border px-2 text-[12px] leading-none transition-colors"
          >
            <span>{t("overview.focusSession")}</span>
            <ArrowUpRight className="size-3" />
          </button>
          {onClose && (
            <button
              type="button"
              aria-label={t("common.close")}
              onClick={onClose}
              className="text-ink-tertiary hover:text-ink-primary hover:bg-accent inline-flex size-[26px] cursor-pointer items-center justify-center rounded-[6px] transition-colors"
            >
              <X className="size-3.5" />
            </button>
          )}
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
        viewportClassName="px-4 pt-4 pb-[24px]"
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
                        agentId={agentId}
                        thinkingSignature=""
                        getThinkingDurationMs={NOOP_THINKING_DURATION}
                        holdViewport={holdViewport}
                        onRequestRewind={NOOP_REWIND}
                        onPreviewImage={setImagePreviewTarget}
                        sessionSubagents={sessionSubagents}
                        sessionSubagentMatcher={nameMatcher}
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
              {foregroundRunning ? t("stage.working") : t("overview.noTurnsYet")}
            </div>
          )}
        </div>
      </ConversationViewport>

      {/* Actionable Pending Approval / Clarification Dock */}
      {hasPendingDecisions && hostId && agentId && (
        <div className="bg-surface-base relative z-20 shrink-0 pt-2">
          {pendingPlanApproval && (
            <PlanReviewCard
              hostId={hostId}
              agentId={agentId}
              cwd={cwd}
              plan={pendingPlanApproval}
              compact
            />
          )}
          <DecisionDock
            hostId={hostId}
            agentId={agentId}
            cwd={cwd}
            permissions={pendingPermissions}
            questions={pendingQuestions}
            subagentPermissions={subagentPermissions}
            registerGlobalShortcuts={false}
            compact
          />
        </div>
      )}

      {/* Quick Inline Reply Bar */}
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
            placeholder={t("overview.messageAgentPlaceholder", { name: handleLabel })}
            className="text-ink-primary placeholder:text-ink-faint min-w-0 flex-1 bg-transparent text-[13px] leading-[18px] outline-none"
          />
          {foregroundRunning && !reply.trim() ? (
            <button
              type="button"
              title={t("overview.stopAgent")}
              onClick={() => void orbitIpc.interruptAgent(hostId, agentId)}
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
