import { memo, useMemo } from "react";
import {
  AlertTriangle,
  ArrowUpRight,
  Loader2,
  MessageCircleQuestion,
  Trash2,
} from "lucide-react";
import {
  ConversationMessageFooter,
  ConversationUserBubble,
  ConversationViewport,
} from "@/components/conversation/conversation-viewport";
import { ConversationComposer } from "@/components/conversation/conversation-composer";
import { MarkdownBlocksView } from "@/components/markdown-blocks";
import {
  type SideQuestionEntry,
  orbitActions,
  sideQuestionScopeKey,
  useOrbitSelector,
} from "@/state/orbit-store";

const EMPTY_ENTRIES: readonly SideQuestionEntry[] = [];

export const SideQuestionTab = memo(function SideQuestionTab() {
  const scope = useOrbitSelector((s) => sideQuestionScopeKey(s));
  const hostId = useOrbitSelector((s) => s.selectedHostId);
  const cwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const enterToSend = useOrbitSelector((s) => s.enterToSend);
  const entries = useOrbitSelector((s) => s.sideQuestionsByScope[scope] ?? EMPTY_ENTRIES);
  const draft = useOrbitSelector((s) => s.sideQuestionDrafts[scope] ?? "");
  const focusNonce = useOrbitSelector((s) => s.sideQuestionFocusNonce);
  const activeProviderLabel = useOrbitSelector((s) => {
    if (!s.selectedHostId) return null;
    if (!s.isDraftNewSession && s.selectedAgentId) {
      const ag = s.hostDetails[s.selectedHostId]?.agents.find(
        (a) => a.agent_id === s.selectedAgentId,
      );
      if (ag?.provider_label) return ag.provider_label;
    }
    return s.draftProvider === "claude_code"
      ? "Claude Code"
      : s.draftProvider === "antigravity"
        ? "Antigravity"
        : "Codex";
  });

  const isStreaming = entries.some((e) => e.status === "streaming");
  const promptHistory = useMemo(() => entries.map((e) => e.question), [entries]);
  const lastEntry = entries[entries.length - 1];
  const contentVersion = `${entries.length}:${lastEntry?.id ?? ""}:${lastEntry?.answer.length ?? 0}:${lastEntry?.status ?? ""}`;

  const handleSubmit = () => {
    const trimmed = draft.trim();
    if (!trimmed || isStreaming) return;
    void orbitActions.submitSideQuestion(trimmed);
  };

  return (
    <div className="orbit-stage-enter flex min-h-0 flex-1 flex-col overflow-hidden">
      {/* Quiet Context Sub-Bar */}
      <div className="border-border flex h-[36px] shrink-0 items-center justify-between gap-2 border-b px-3">
        <div className="text-muted-foreground flex min-w-0 items-center gap-1.5 text-[11.5px]">
          <span className="font-mono text-foreground/75">/btw</span>
          <span className="truncate">Read-only · multi-turn side thread</span>
        </div>

        {entries.length > 0 && (
          <button
            type="button"
            title="Clear side questions"
            onClick={() => orbitActions.clearSideQuestions()}
            className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[24px] cursor-pointer items-center gap-1 rounded-md px-2 text-[11.5px] transition-colors"
          >
            <Trash2 className="h-[12px] w-[12px]" />
            <span>Clear</span>
          </button>
        )}
      </div>

      {/* Shared Conversation Viewport matching main StreamStage prose & bubbles */}
      <ConversationViewport
        hostId={hostId}
        cwd={cwd}
        resetKey={scope}
        contentVersion={contentVersion}
        viewportClassName="px-5 py-5"
      >
        {entries.length === 0 ? (
          <div className="flex min-h-[260px] flex-col items-center justify-center gap-3 px-4 text-center">
            <div className="bg-muted text-muted-foreground inline-flex h-10 w-10 items-center justify-center rounded-xl">
              <MessageCircleQuestion className="h-[19px] w-[19px] stroke-[1.65]" />
            </div>
            <div className="space-y-1">
              <div className="text-foreground text-[14px] font-medium">
                Ask a Side Question
              </div>
              <p className="text-muted-foreground max-w-[280px] text-[12.5px] leading-relaxed">
                Ask anything about the active conversation while the main agent keeps working. Type{" "}
                <code className="bg-muted text-foreground/85 rounded px-1 py-0.5 font-mono text-[11.5px]">
                  /btw
                </code>{" "}
                in the main composer or ask below.
              </p>
            </div>
          </div>
        ) : (
          <div className="space-y-7">
            {entries.map((entry) => (
              <div key={entry.id} className="orbit-bubble-enter space-y-4">
                <ConversationUserBubble text={entry.question} />

                {/* Assistant Side Answer (full primary conversation typography) */}
                <div className="space-y-2">
                  {entry.status === "streaming" && !entry.answer ? (
                    <div className="text-muted-foreground flex items-center gap-2 py-1 text-[13.5px]">
                      <Loader2 className="h-[14px] w-[14px] animate-spin" />
                      <span className="agent-text-shimmer">
                        Thinking with {entry.providerLabel}…
                      </span>
                    </div>
                  ) : (
                    <>
                      {entry.answer && (
                        <div className="min-w-0 text-[14.5px] leading-[1.68]">
                          {entry.blocks.length > 0 ? (
                            <MarkdownBlocksView
                              blocks={entry.blocks}
                              streaming={entry.status === "streaming"}
                            />
                          ) : (
                            <div className="selectable-text text-foreground/92 leading-[1.68] whitespace-pre-wrap">
                              {entry.answer}
                            </div>
                          )}
                        </div>
                      )}
                      {entry.error && (
                        <div className="border-rose-500/25 bg-rose-500/[0.05] text-foreground/85 flex items-start gap-2 rounded-xl border px-3 py-2 text-[12.5px]">
                          <AlertTriangle className="mt-0.5 h-[14px] w-[14px] shrink-0 text-rose-500" />
                          <span className="selectable-text break-words">{entry.error}</span>
                        </div>
                      )}
                    </>
                  )}

                  {entry.status === "done" && entry.answer.trim() && (
                    <ConversationMessageFooter
                      copyText={entry.answer}
                      metaLabel={`${entry.providerLabel} · side thread`}
                      rightSlot={
                        <button
                          type="button"
                          onClick={() =>
                            orbitActions.promoteSideQuestionToComposer(
                              `Regarding "${entry.question}":\n${entry.answer}`,
                            )
                          }
                          className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex cursor-pointer items-center gap-1 rounded-md px-1.5 py-0.5 text-[11.5px] transition-colors"
                        >
                          <span>Send to main chat</span>
                          <ArrowUpRight className="h-[12px] w-[12px]" />
                        </button>
                      }
                    />
                  )}
                </div>
              </div>
            ))}
          </div>
        )}
      </ConversationViewport>

      {/* Shared Conversation Composer matching primary OmniComposer */}
      <ConversationComposer
        variant="docked"
        value={draft}
        onChange={(next) => orbitActions.setSideQuestionDraft(next)}
        onSubmit={handleSubmit}
        onInterrupt={isStreaming ? () => orbitActions.cancelSideQuestion(scope) : undefined}
        isBusy={isStreaming}
        enterToSend={enterToSend}
        focusNonce={focusNonce}
        promptHistory={promptHistory}
        placeholder={
          activeProviderLabel
            ? `Ask ${activeProviderLabel} on the side…`
            : "Ask a side question…"
        }
        leftControlsSlot={
          activeProviderLabel ? (
            <span className="text-muted-foreground/80 truncate px-1.5 text-[12px]">
              {activeProviderLabel}
            </span>
          ) : null
        }
      />
    </div>
  );
});
