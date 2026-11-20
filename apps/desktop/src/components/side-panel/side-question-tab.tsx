import { memo, useEffect, useRef } from "react";
import {
  AlertTriangle,
  ArrowUp,
  ArrowUpRight,
  Loader2,
  MessageCircleQuestion,
  Trash2,
} from "lucide-react";
import { MarkdownBlocksView } from "@/components/markdown-blocks";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useImeCompositionGuard } from "@/lib/ime";
import {
  type SideQuestionEntry,
  orbitActions,
  sideQuestionScopeKey,
  useOrbitSelector,
} from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const EMPTY_ENTRIES: readonly SideQuestionEntry[] = [];

export const SideQuestionTab = memo(function SideQuestionTab() {
  const scope = useOrbitSelector((s) => sideQuestionScopeKey(s));
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

  const inputRef = useRef<HTMLTextAreaElement | null>(null);
  const bottomRef = useRef<HTMLDivElement | null>(null);
  const ime = useImeCompositionGuard();

  useEffect(() => {
    requestAnimationFrame(() => inputRef.current?.focus());
  }, [focusNonce]);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [entries.length, entries[entries.length - 1]?.answer.length]);

  const isStreaming = entries.some((e) => e.status === "streaming");

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
          <span className="truncate">Read-only · never alters main thread</span>
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

      {/* Ephemeral Q&A Stream */}
      <ScrollArea className="bg-canvas min-h-0 flex-1" viewportClassName="px-4 py-4">
        {entries.length === 0 ? (
          <div className="flex min-h-[240px] flex-col items-center justify-center gap-3 px-4 text-center">
            <div className="bg-muted text-muted-foreground inline-flex h-9 w-9 items-center justify-center rounded-xl">
              <MessageCircleQuestion className="h-[18px] w-[18px] stroke-[1.65]" />
            </div>
            <div className="space-y-1">
              <div className="text-foreground text-[13px] font-medium">
                Ask a Side Question
              </div>
              <p className="text-muted-foreground max-w-[260px] text-[12px] leading-relaxed">
                Ask anything about the active conversation while the main agent keeps working. Type{" "}
                <code className="bg-muted text-foreground/85 rounded px-1 py-0.5 font-mono text-[11px]">
                  /btw
                </code>{" "}
                in the main composer or ask below.
              </p>
            </div>
          </div>
        ) : (
          <div className="space-y-5">
            {entries.map((entry) => (
              <div key={entry.id} className="orbit-bubble-enter space-y-2.5">
                {/* User Side Question */}
                <div className="flex justify-end">
                  <div className="prompt-bubble-surface selectable-text text-foreground max-w-[90%] rounded-2xl rounded-tr-md px-3.5 py-2 text-[12.5px] leading-relaxed whitespace-pre-wrap">
                    {entry.question}
                  </div>
                </div>

                {/* Assistant Side Answer */}
                <div className="space-y-1.5">
                  {entry.status === "streaming" && !entry.answer ? (
                    <div className="text-muted-foreground flex items-center gap-2 py-1 text-[12.5px]">
                      <Loader2 className="h-[13.5px] w-[13.5px] animate-spin" />
                      <span className="agent-text-shimmer">
                        Thinking with {entry.providerLabel}…
                      </span>
                    </div>
                  ) : (
                    <>
                      {entry.answer && (
                        <div className="text-[13px]">
                          {entry.blocks.length > 0 ? (
                            <MarkdownBlocksView
                              blocks={entry.blocks}
                              streaming={entry.status === "streaming"}
                              compact
                            />
                          ) : (
                            <div className="selectable-text text-foreground/90 leading-relaxed whitespace-pre-wrap">
                              {entry.answer}
                            </div>
                          )}
                        </div>
                      )}
                      {entry.error && (
                        <div className="border-rose-500/25 bg-rose-500/[0.05] text-foreground/85 flex items-start gap-2 rounded-xl border px-3 py-2 text-[12px]">
                          <AlertTriangle className="mt-0.5 h-[14px] w-[14px] shrink-0 text-rose-500" />
                          <span className="selectable-text break-words">{entry.error}</span>
                        </div>
                      )}
                    </>
                  )}

                  {entry.status === "done" && entry.answer.trim() && (
                    <div className="flex items-center justify-between pt-0.5">
                      <span className="text-muted-foreground/75 text-[11px]">
                        {entry.providerLabel} · ephemeral
                      </span>
                      <button
                        type="button"
                        onClick={() =>
                          orbitActions.promoteSideQuestionToComposer(
                            `Regarding "${entry.question}":\n${entry.answer}`,
                          )
                        }
                        className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex cursor-pointer items-center gap-1 rounded-md px-1.5 py-0.5 text-[11px] transition-colors"
                      >
                        <span>Send to main chat</span>
                        <ArrowUpRight className="h-[11.5px] w-[11.5px]" />
                      </button>
                    </div>
                  )}
                </div>
              </div>
            ))}
            <div ref={bottomRef} />
          </div>
        )}
      </ScrollArea>

      {/* Compact Side Input */}
      <div className="border-border bg-sidebar shrink-0 border-t p-2.5">
        <div className="composer-surface flex items-end gap-2 rounded-xl px-3 py-2">
          <textarea
            ref={inputRef}
            rows={1}
            value={draft}
            onChange={(e) => orbitActions.setSideQuestionDraft(e.target.value)}
            {...ime.compositionProps}
            onKeyDown={(e) => {
              if (ime.shouldIgnoreKeyDown(e)) return;
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                handleSubmit();
              }
            }}
            placeholder={
              activeProviderLabel
                ? `Ask ${activeProviderLabel} on the side…`
                : "Ask a side question…"
            }
            className="text-foreground placeholder:text-muted-foreground max-h-28 min-h-[22px] flex-1 resize-none bg-transparent text-[12.5px] leading-[1.45] outline-none"
          />
          <button
            type="button"
            disabled={!draft.trim() || isStreaming}
            onClick={handleSubmit}
            aria-label="Send side question"
            className={cn(
              "orbit-press inline-flex h-[26px] w-[26px] shrink-0 cursor-pointer items-center justify-center rounded-lg transition-colors",
              draft.trim() && !isStreaming
                ? "bg-foreground text-background hover:opacity-90"
                : "bg-muted text-muted-foreground cursor-default opacity-60",
            )}
          >
            {isStreaming ? (
              <Loader2 className="h-[13.5px] w-[13.5px] animate-spin" />
            ) : (
              <ArrowUp className="h-[14px] w-[14px] stroke-[2.2]" />
            )}
          </button>
        </div>
      </div>
    </div>
  );
});
