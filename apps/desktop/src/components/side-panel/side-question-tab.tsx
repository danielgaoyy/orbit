import { memo, useMemo } from "react";
import { AlertTriangle, ArrowUpRight, Loader2, Plus, Trash2 } from "lucide-react";
import { ComposerModePicker } from "@/components/composer/composer-mode-picker";
import { ComposerModelPicker } from "@/components/composer/composer-model-picker";
import { ConversationComposer } from "@/components/conversation/conversation-composer";
import {
  ConversationMessageFooter,
  ConversationUserBubble,
  ConversationViewport,
} from "@/components/conversation/conversation-viewport";
import { MarkdownBlocksView } from "@/components/markdown-blocks";
import type { SessionMode } from "@/lib/orbit-ipc";
import {
  type SideQuestionEntry,
  orbitActions,
  sideQuestionScopeKey,
  useOrbitSelector,
} from "@/state/orbit-store";

const EMPTY_ENTRIES: readonly SideQuestionEntry[] = [];

const QUICK_SIDE_PROMPTS = [
  "Summarize what changed so far",
  "Explain the latest output",
  "Check for potential edge cases",
] as const;

export const SideQuestionTab = memo(function SideQuestionTab() {
  const scope = useOrbitSelector((s) => sideQuestionScopeKey(s));
  const hostId = useOrbitSelector((s) => s.selectedHostId);
  const cwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const enterToSend = useOrbitSelector((s) => s.enterToSend);
  const entries = useOrbitSelector((s) => s.sideQuestionsByScope[scope] ?? EMPTY_ENTRIES);
  const draft = useOrbitSelector((s) => s.sideQuestionDrafts[scope] ?? "");
  const focusNonce = useOrbitSelector((s) => s.sideQuestionFocusNonce);

  const activeProviderLabel = useOrbitSelector((s) => {
    if (!s.selectedHostId) return "Assistant";
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

  const activeMode: SessionMode = useOrbitSelector((s) => {
    if (!s.isDraftNewSession && s.selectedHostId && s.selectedAgentId) {
      const ag = s.hostDetails[s.selectedHostId]?.agents.find(
        (a) => a.agent_id === s.selectedAgentId,
      );
      if (ag) return ag.mode;
    }
    return s.draftMode;
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
      {/* Shared Conversation Viewport matching main StreamStage prose & bubbles */}
      <ConversationViewport
        hostId={hostId}
        cwd={cwd}
        resetKey={scope}
        contentVersion={contentVersion}
        viewportClassName="px-5 py-5"
        contentClassName={entries.length === 0 ? "flex min-h-full flex-col" : undefined}
      >
        {entries.length === 0 ? (
          <div className="flex flex-1 flex-col items-center justify-center px-4 py-8 text-center">
            <div className="max-w-[300px] space-y-5">
              <div className="space-y-1.5">
                <h3 className="text-foreground text-[15px] font-medium tracking-[-0.015em]">
                  Side Question
                </h3>
                <p className="text-muted-foreground text-[13px] leading-[1.55]">
                  Ask in context without interrupting or modifying the active thread.
                </p>
              </div>

              <div className="flex flex-col items-center gap-1.5">
                {QUICK_SIDE_PROMPTS.map((prompt) => (
                  <button
                    key={prompt}
                    type="button"
                    onClick={() => void orbitActions.submitSideQuestion(prompt)}
                    className="border-border/80 bg-elevated/45 text-foreground/80 hover:border-foreground/20 hover:bg-accent/70 hover:text-foreground inline-flex max-w-full cursor-pointer items-center rounded-full border px-3 py-1 text-[12px] transition-colors"
                  >
                    <span className="truncate">{prompt}</span>
                  </button>
                ))}
              </div>
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
                      metaLabel={entry.providerLabel}
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

      {/* Shared ConversationComposer reusing main ComposerModelPicker & ComposerModePicker (read-only disabled) */}
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
        placeholder={`Ask ${activeProviderLabel} on the side…`}
        leftControlsSlot={
          <>
            <button
              type="button"
              disabled
              title="Attachments are disabled in side questions"
              className="text-foreground/35 inline-flex size-8.5 shrink-0 cursor-not-allowed items-center justify-center rounded-full"
            >
              <Plus className="size-[18px] stroke-[1.75]" />
            </button>
            <ComposerModelPicker
              disabled
              disabledTitle={`Inherits active session model & thinking effort (${activeProviderLabel})`}
            />
          </>
        }
        rightControlsSlot={(widthPx) => (
          <>
            {entries.length > 0 && (
              <button
                type="button"
                title="Clear side thread"
                onClick={() => orbitActions.clearSideQuestions()}
                className="text-muted-foreground hover:text-foreground inline-flex size-8 cursor-pointer items-center justify-center rounded-full transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
              >
                <Trash2 className="size-[14px]" />
              </button>
            )}
            <ComposerModePicker
              mode={activeMode}
              disabled
              iconOnly={widthPx > 0 && widthPx < 360}
              disabledTitle="Read-only in side questions — inherits context without modifying files"
            />
          </>
        )}
      />
    </div>
  );
});
