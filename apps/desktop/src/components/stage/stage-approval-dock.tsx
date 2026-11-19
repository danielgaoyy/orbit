import { memo, useState } from "react";
import { HelpCircle, ShieldAlert } from "lucide-react";
import { type QuestionItem, type TimelineItemView, orbitIpc } from "@/lib/orbit-ipc";
import { modSymbol } from "@/lib/keybindings";
import { ShortcutKbd } from "@/components/ui/overlays";
import { cn } from "@/lib/utils";

function StickyQuestionDockCard({
  hostId,
  agentId,
  requestId,
  questions,
}: {
  hostId: string;
  agentId: string;
  requestId: string;
  questions: QuestionItem[];
}) {
  const [selections, setSelections] = useState<Record<number, string[]>>(() => {
    const init: Record<number, string[]> = {};
    questions.forEach((q, idx) => {
      if (q.options.length > 0) init[idx] = [q.options[0]];
    });
    return init;
  });

  const toggleOption = (qIdx: number, opt: string, multi: boolean) => {
    setSelections((prev) => {
      const cur = prev[qIdx] ?? [];
      if (!multi) return { ...prev, [qIdx]: [opt] };
      return {
        ...prev,
        [qIdx]: cur.includes(opt) ? cur.filter((x) => x !== opt) : [...cur, opt],
      };
    });
  };

  const handleSubmit = () => {
    const ordered = questions.map((_, idx) => selections[idx] ?? []);
    void orbitIpc.answerQuestion(hostId, agentId, requestId, ordered);
  };

  return (
    <div className="bg-elevated orbit-dock-enter rounded-2xl border border-amber-500/35 p-4 shadow-sm">
      <div className="mb-2 flex items-center gap-1.5 text-[13px] font-medium text-amber-700 dark:text-amber-300">
        <HelpCircle className="size-4" />
        <span>Agent needs clarification</span>
      </div>
      <div className="space-y-3">
        {questions.map((q, qIdx) => (
          <div key={q.id || qIdx} className="space-y-1.5">
            <div className="selectable-text text-foreground text-[13.5px] font-medium">
              {q.question}
            </div>
            <div className="flex flex-wrap gap-1.5">
              {q.options.map((opt, optIdx) => {
                const chosen = selections[qIdx]?.includes(opt);
                return (
                  <button
                    key={opt}
                    type="button"
                    onClick={() => toggleOption(qIdx, opt, q.is_multi_select)}
                    className={cn(
                      "inline-flex cursor-pointer items-center gap-1.5 rounded-lg border px-3 py-1.5 text-[13px] transition-colors",
                      chosen
                        ? "border-amber-500/50 bg-amber-500/15 text-foreground font-medium"
                        : "border-border bg-background/70 text-muted-foreground hover:text-foreground",
                    )}
                  >
                    <span className="font-mono text-[11px] opacity-60">{optIdx + 1}</span>
                    <span>{opt}</span>
                  </button>
                );
              })}
            </div>
          </div>
        ))}
        <div className="flex justify-end">
          <button
            type="button"
            onClick={handleSubmit}
            className="bg-foreground text-background inline-flex h-8 cursor-pointer items-center rounded-lg px-3.5 text-[13px] font-medium transition-opacity hover:opacity-90"
          >
            Submit Response
          </button>
        </div>
      </div>
    </div>
  );
}

export const StickyApprovalDock = memo(
  function StickyApprovalDock({
    hostId,
    agentId,
    permissions,
    questions,
  }: {
    hostId: string;
    agentId: string;
    permissions: Extract<TimelineItemView, { item_type: "permission_request" }>[];
    questions: Extract<TimelineItemView, { item_type: "question_request" }>[];
  }) {
    const mod = modSymbol();
    if (permissions.length === 0 && questions.length === 0) return null;

    return (
      <div className="w-full px-[24px] pb-2.5">
        <div className="mx-auto w-full max-w-[768px] space-y-2">
          {permissions.map((perm, idx) => {
            const decisions =
              perm.allowed_decisions.length > 0
                ? perm.allowed_decisions
                : ["allow", "allow_always", "deny"];
            const targetPreview =
              perm.detail.kind === "shell"
                ? `$ ${perm.detail.command}`
                : perm.detail.kind === "file_edit" || perm.detail.kind === "file_read"
                  ? perm.detail.path
                  : JSON.stringify(perm.detail);

            return (
              <div
                key={perm.request_id}
                className="bg-elevated orbit-dock-enter flex flex-col gap-2.5 rounded-2xl border border-amber-500/35 px-4 py-3 shadow-xs sm:flex-row sm:items-center sm:justify-between"
              >
                <div className="min-w-0 space-y-0.5">
                  <div className="flex items-center gap-1.5 text-[13px] font-medium text-amber-700 dark:text-amber-300">
                    <ShieldAlert className="size-4 shrink-0" />
                    <span className="truncate">{perm.title}</span>
                  </div>
                  <div className="selectable-text text-foreground/90 truncate font-mono text-[12px]">
                    {targetPreview}
                  </div>
                </div>

                <div className="flex shrink-0 items-center gap-1.5">
                  {decisions.map((dec) => {
                    const isDeny = dec.toLowerCase().includes("deny");
                    const isAlways = dec.toLowerCase().includes("always");
                    return (
                      <button
                        key={dec}
                        type="button"
                        onClick={() =>
                          void orbitIpc.resolvePermission(hostId, agentId, perm.request_id, dec)
                        }
                        className={cn(
                          "orbit-press inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-lg border px-3 text-[12.5px] font-medium",
                          isDeny
                            ? "border-rose-500/30 bg-rose-500/10 text-rose-600 hover:bg-rose-500/20 dark:text-rose-300"
                            : isAlways
                              ? "border-border bg-secondary text-secondary-foreground hover:bg-accent"
                              : "bg-foreground text-background border-transparent hover:opacity-90",
                        )}
                      >
                        <span>{dec.replace("_", " ")}</span>
                        {idx === 0 && !isDeny && !isAlways && (
                          <ShortcutKbd
                            keys={`${mod}Y`}
                            kbdClassName="bg-background/20 text-background dark:bg-background/20"
                          />
                        )}
                        {idx === 0 && isAlways && <ShortcutKbd keys={`${mod}⇧Y`} />}
                        {idx === 0 && isDeny && (
                          <ShortcutKbd
                            keys={`${mod}⌫`}
                            kbdClassName="bg-rose-500/15 text-rose-600 dark:bg-rose-500/20 dark:text-rose-200"
                          />
                        )}
                      </button>
                    );
                  })}
                </div>
              </div>
            );
          })}

          {questions.map((q) => (
            <StickyQuestionDockCard
              key={q.request_id}
              hostId={hostId}
              agentId={agentId}
              requestId={q.request_id}
              questions={q.questions}
            />
          ))}
        </div>
      </div>
    );
  },
  (prev, next) =>
    prev.hostId === next.hostId &&
    prev.agentId === next.agentId &&
    prev.permissions.length === next.permissions.length &&
    prev.questions.length === next.questions.length &&
    prev.permissions[0]?.request_id === next.permissions[0]?.request_id &&
    prev.questions[0]?.request_id === next.questions[0]?.request_id,
);
