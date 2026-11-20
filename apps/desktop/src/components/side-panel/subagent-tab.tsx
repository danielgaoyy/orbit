import { memo } from "react";
import { ArrowUpRight, Check, Layers, Loader2, AlertTriangle } from "lucide-react";
import { MarkdownBlocksView } from "@/components/markdown-blocks";
import { ScrollArea } from "@/components/ui/scroll-area";
import { type SidePanelTab, orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

export const SubagentTab = memo(function SubagentTab({ tab }: { tab: SidePanelTab }) {
  const hostId = useOrbitSelector((s) => tab.hostId ?? s.selectedHostId);
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
  const parentSubagent = useOrbitSelector((s) => {
    if (!s.selectedHostId || !s.selectedAgentId || !tab.subagentCallId) return null;
    const parentStage = s.stages[`${s.selectedHostId}::${s.selectedAgentId}`];
    return parentStage?.insights.subagents.find((sa) => sa.id === tab.subagentCallId) ?? null;
  });

  const status = childAgent
    ? childAgent.is_running
      ? "running"
      : childAgent.status_label.toLowerCase().includes("error")
        ? "failed"
        : "completed"
    : (parentSubagent?.status ?? "completed");

  return (
    <div className="orbit-stage-enter flex min-h-0 flex-1 flex-col overflow-hidden">
      {/* Subagent Header Sub-Bar */}
      <div className="border-border flex h-[36px] shrink-0 items-center justify-between gap-2 border-b px-3">
        <div className="flex min-w-0 items-center gap-2">
          <Layers className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0" />
          <span className="text-foreground truncate text-[12.5px] font-medium">
            {tab.subagentRole || childAgent?.title || "Subagent"}
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
            onClick={() => orbitActions.selectAgent(hostId, tab.childAgentId!)}
            className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[24px] shrink-0 cursor-pointer items-center gap-1 rounded-md px-2 text-[11.5px] transition-colors"
          >
            <span>Open thread</span>
            <ArrowUpRight className="h-[12px] w-[12px]" />
          </button>
        )}
      </div>

      {/* Subagent Prompt & Child Activity */}
      <ScrollArea className="bg-canvas min-h-0 flex-1" viewportClassName="space-y-4 p-4">
        {tab.subagentPrompt && (
          <div className="space-y-1.5">
            <div className="text-muted-foreground text-[11px] font-medium tracking-wide uppercase">
              Delegated Task
            </div>
            <div className="border-border bg-elevated/60 selectable-text text-foreground/88 rounded-xl border p-3 text-[12.5px] leading-relaxed whitespace-pre-wrap">
              {tab.subagentPrompt}
            </div>
          </div>
        )}

        {childStage && childStage.turns.length > 0 ? (
          <div className="space-y-4">
            <div className="text-muted-foreground text-[11px] font-medium tracking-wide uppercase">
              Activity ({childStage.turns.length} {childStage.turns.length === 1 ? "turn" : "turns"})
            </div>
            {childStage.turns.map((turn) => (
              <div key={turn.key} className="border-border space-y-2 border-l-2 pl-3">
                {turn.work_summary_label && (
                  <div className="text-muted-foreground text-[12px]">
                    {turn.work_summary_label}
                  </div>
                )}
                {turn.terminal_reply?.item_type === "assistant_message" && (
                  <div className="text-[12.5px]">
                    <MarkdownBlocksView
                      blocks={turn.terminal_reply.blocks}
                      streaming={turn.terminal_reply.streaming}
                      compact
                    />
                  </div>
                )}
              </div>
            ))}
          </div>
        ) : (
          <div className="border-border text-muted-foreground rounded-xl border border-dashed px-4 py-6 text-center text-[12px]">
            {status === "running"
              ? "Subagent is running in an isolated context. Final results will return to the main thread."
              : "Subagent execution completed. Summary and outputs are folded into the parent conversation."}
          </div>
        )}
      </ScrollArea>
    </div>
  );
});
