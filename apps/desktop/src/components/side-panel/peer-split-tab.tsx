import { memo, useEffect, useRef, useState } from "react";
import {
  ArrowUp,
  ArrowUpRight,
  FileText,
  GitBranch,
  Loader2,
  Square,
} from "lucide-react";
import { useImeCompositionGuard } from "@/lib/ime";
import { orbitIpc, semanticAgentTabLabel } from "@/lib/orbit-ipc";
import { MarkdownBlocksView } from "@/components/markdown-blocks";
import { ProviderIcon } from "@/components/ui/provider-icons";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  type SidePanelTab,
  orbitActions,
  threadScopeKey,
  useOrbitSelector,
} from "@/state/orbit-store";

export const PeerSplitTab = memo(function PeerSplitTab({ tab }: { tab: SidePanelTab }) {
  const hostId = tab.hostId ?? "";
  const peerAgentId = tab.peerAgentId ?? "";
  const tKey = threadScopeKey(hostId, peerAgentId);

  const stage = useOrbitSelector((s) => s.stages[tKey] ?? null);
  const hostDetail = useOrbitSelector((s) => (hostId ? (s.hostDetails[hostId] ?? null) : null));
  const agent =
    hostDetail?.agents.find((a) => a.agent_id === peerAgentId) ?? stage?.agent ?? null;
  const workspace = agent
    ? hostDetail?.workspaces.find((w) => w.workspace_id === agent.workspace_id) ?? null
    : null;

  const [reply, setReply] = useState("");
  const [sending, setSending] = useState(false);
  const replyIme = useImeCompositionGuard();
  const bottomRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!hostId || !peerAgentId) return;
    void orbitIpc.openAgentTimeline(hostId, peerAgentId);
    void orbitActions.pullStageDelta(hostId, peerAgentId, true);
  }, [hostId, peerAgentId]);

  const turns = stage?.turns ?? [];

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [turns.length, stage?.latest_seq]);

  const handleSend = async () => {
    const text = reply.trim();
    if (!text || !hostId || !peerAgentId || sending) return;
    setSending(true);
    setReply("");
    try {
      await orbitIpc.sendPrompt(hostId, peerAgentId, text, {
        displayPrompt: text,
        activeTurnBehavior: agent?.is_running ? "steer" : "queue",
      });
    } finally {
      setSending(false);
    }
  };

  const handleLabel = agent ? semanticAgentTabLabel(agent) : tab.title;

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      {/* Split Header */}
      <div className="border-border/70 flex items-center justify-between gap-2 border-b px-3 py-2">
        <div className="flex min-w-0 items-center gap-2">
          {agent?.is_running ? (
            <Loader2 className="h-[13.5px] w-[13.5px] shrink-0 animate-spin text-sky-500" />
          ) : agent ? (
            <ProviderIcon
              provider={agent.provider}
              className="h-[14px] w-[14px] shrink-0 opacity-80"
            />
          ) : null}
          <div className="min-w-0">
            <div className="flex items-center gap-1.5">
              <span className="text-foreground truncate text-[13px] font-semibold">
                {handleLabel}
              </span>
              {agent?.worktree_branch && (
                <span className="bg-muted text-muted-foreground inline-flex items-center gap-0.5 rounded px-1.5 py-0.5 font-mono text-[10.5px]">
                  <GitBranch className="h-[10px] w-[10px]" />
                  {agent.worktree_branch}
                </span>
              )}
            </div>
            <div className="text-muted-foreground truncate text-[11px]">
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
              className="text-muted-foreground hover:text-foreground inline-flex h-[24px] cursor-pointer items-center gap-1 rounded-md px-1.5 text-[11.5px] hover:bg-black/[0.05] dark:hover:bg-white/[0.07]"
            >
              <FileText className="h-[12px] w-[12px]" />
            </button>
          )}
          <button
            type="button"
            onClick={() => orbitActions.selectAgent(hostId, peerAgentId)}
            className="border-border hover:bg-accent inline-flex h-[24px] cursor-pointer items-center gap-1 rounded-md border px-2 text-[11.5px]"
          >
            <span>Focus</span>
            <ArrowUpRight className="h-[11.5px] w-[11.5px]" />
          </button>
        </div>
      </div>

      {/* Live Peer Transcript */}
      <ScrollArea className="min-h-0 flex-1" viewportClassName="p-3.5 space-y-4 select-text">
        {turns.length === 0 ? (
          <div className="text-muted-foreground py-8 text-center text-[12.5px]">
            {agent?.is_running ? "Peer is working…" : "No turns yet."}
          </div>
        ) : (
          turns.map((turn) => {
            const promptText =
              turn.user_prompt?.item_type === "user_prompt"
                ? turn.user_prompt.text
                : turn.agent_message?.item_type === "agent_message"
                  ? turn.agent_message.text
                  : null;
            return (
              <div key={turn.key} className="space-y-2.5">
                {promptText && (
                  <div className="bg-muted/70 text-foreground rounded-xl px-3 py-2 text-[12.5px] leading-relaxed whitespace-pre-wrap break-words">
                    {promptText}
                  </div>
                )}

                {turn.work_counts.action_steps > 0 && (
                  <div className="text-muted-foreground flex items-center gap-1.5 text-[11.5px]">
                    {turn.has_active_item && (
                      <Loader2 className="h-[11px] w-[11px] animate-spin text-sky-500" />
                    )}
                    <span>
                      {turn.has_active_item
                        ? turn.work_summary_live_label
                        : turn.work_summary_label}
                    </span>
                  </div>
                )}

                {turn.live_segments.map((seg) => {
                  if (
                    seg.kind === "assistant_message" &&
                    seg.item.item_type === "assistant_message"
                  ) {
                    return (
                      <div
                        key={seg.key}
                        className="text-foreground/92 text-[13px] leading-relaxed"
                      >
                        {seg.item.blocks.length > 0 ? (
                          <MarkdownBlocksView blocks={seg.item.blocks} />
                        ) : (
                          <div className="whitespace-pre-wrap break-words">{seg.item.text}</div>
                        )}
                      </div>
                    );
                  }
                  return null;
                })}
              </div>
            );
          })
        )}
        <div ref={bottomRef} />
      </ScrollArea>

      {/* Quick Peer Reply Bar */}
      <div className="border-border/70 border-t p-2.5">
        <div className="border-border bg-elevated/55 flex items-center gap-1.5 rounded-xl border px-2.5 py-1.5">
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
            className="text-foreground placeholder:text-muted-foreground/60 min-w-0 flex-1 bg-transparent text-[12.5px] outline-none"
          />
          {agent?.is_running && !reply.trim() ? (
            <button
              type="button"
              title="Stop peer"
              onClick={() => void orbitIpc.interruptAgent(hostId, peerAgentId)}
              className="inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-full bg-rose-500/15 text-rose-500 hover:bg-rose-500/25"
            >
              <Square className="h-[10px] w-[10px] fill-current" />
            </button>
          ) : (
            <button
              type="button"
              disabled={!reply.trim() || sending}
              onClick={() => void handleSend()}
              className="bg-foreground text-background inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-full disabled:opacity-35"
            >
              <ArrowUp className="h-[13px] w-[13px] stroke-[2.15]" />
            </button>
          )}
        </div>
      </div>
    </div>
  );
});
