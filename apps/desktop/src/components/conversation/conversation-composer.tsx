import { memo, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { ArrowUp, Square } from "lucide-react";
import { cn } from "@/lib/utils";

export interface ConversationComposerProps {
  value: string;
  onChange: (next: string) => void;
  onSubmit: () => void;
  onInterrupt?: () => void;
  isBusy?: boolean;
  enterToSend?: boolean;
  placeholder?: string;
  focusNonce?: number;
  promptHistory?: readonly string[];
  variant?: "compact" | "docked";
  topSlot?: ReactNode;
  leftControlsSlot?: ReactNode;
  rightControlsSlot?: ReactNode;
  className?: string;
}

/**
 * Reusable conversation composer matching Orbit's primary composer surface, keyboard semantics
 * (`enterToSend`, `Cmd/Ctrl+Enter`, prompt history recall via ArrowUp/ArrowDown), auto-growing
 * textarea, and busy-state Stop control.
 */
export const ConversationComposer = memo(function ConversationComposer({
  value,
  onChange,
  onSubmit,
  onInterrupt,
  isBusy = false,
  enterToSend = true,
  placeholder = "Ask a follow-up…",
  focusNonce,
  promptHistory,
  variant = "compact",
  topSlot,
  leftControlsSlot,
  rightControlsSlot,
  className,
}: ConversationComposerProps) {
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const [historyIdx, setHistoryIdx] = useState<number>(-1);
  const savedDraftRef = useRef<string>("");

  const minHeight = variant === "compact" ? 34 : 46;
  const maxHeight = variant === "compact" ? 160 : 220;

  useEffect(() => {
    if (focusNonce === undefined) return;
    const raf = requestAnimationFrame(() => textareaRef.current?.focus());
    return () => cancelAnimationFrame(raf);
  }, [focusNonce]);

  useLayoutEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    if (!value) {
      el.style.height = `${minHeight}px`;
      return;
    }
    el.style.height = `${minHeight}px`;
    const nextH = Math.min(maxHeight, Math.max(minHeight, el.scrollHeight));
    el.style.height = `${nextH}px`;
  }, [value, minHeight, maxHeight]);

  const canSend = value.trim().length > 0 && !isBusy;
  const showStop = isBusy && Boolean(onInterrupt) && value.trim().length === 0;

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.nativeEvent.isComposing) return;

    const isModEnter = e.key === "Enter" && (e.metaKey || e.ctrlKey);
    const isPlainEnter = e.key === "Enter" && !e.shiftKey && !e.altKey && !e.metaKey && !e.ctrlKey;
    if (isModEnter || (enterToSend && isPlainEnter)) {
      e.preventDefault();
      if (canSend) {
        setHistoryIdx(-1);
        onSubmit();
      }
      return;
    }

    if (e.key === "Escape" && isBusy && onInterrupt) {
      e.preventDefault();
      onInterrupt();
      return;
    }

    if (promptHistory && promptHistory.length > 0) {
      const el = textareaRef.current;
      if (e.key === "ArrowUp" && el && el.selectionStart === 0 && el.selectionEnd === 0) {
        e.preventDefault();
        if (historyIdx === -1) {
          savedDraftRef.current = value;
        }
        const nextIdx = Math.min(promptHistory.length - 1, historyIdx + 1);
        setHistoryIdx(nextIdx);
        const recalled = promptHistory[promptHistory.length - 1 - nextIdx];
        if (recalled !== undefined) onChange(recalled);
      } else if (
        e.key === "ArrowDown" &&
        historyIdx >= 0 &&
        el &&
        el.selectionStart === value.length
      ) {
        e.preventDefault();
        const nextIdx = historyIdx - 1;
        setHistoryIdx(nextIdx);
        if (nextIdx < 0) {
          onChange(savedDraftRef.current);
        } else {
          const recalled = promptHistory[promptHistory.length - 1 - nextIdx];
          if (recalled !== undefined) onChange(recalled);
        }
      }
    }
  };

  return (
    <div className={cn("border-border bg-sidebar shrink-0 border-t p-2.5", className)}>
      {topSlot}
      <div
        className={cn(
          "composer-surface flex flex-col gap-1.5",
          variant === "compact" ? "rounded-xl px-3 py-2" : "rounded-2xl px-3.5 py-2.5",
        )}
      >
        <textarea
          ref={textareaRef}
          rows={1}
          value={value}
          onChange={(e) => {
            if (historyIdx !== -1) setHistoryIdx(-1);
            onChange(e.target.value);
          }}
          onKeyDown={handleKeyDown}
          placeholder={placeholder}
          style={{ minHeight: `${minHeight}px`, maxHeight: `${maxHeight}px` }}
          className={cn(
            "text-foreground placeholder:text-muted-foreground w-full resize-none bg-transparent leading-[1.48] outline-none",
            variant === "compact" ? "text-[12.5px]" : "text-[13.5px]",
          )}
        />

        <div className="flex items-center justify-between gap-2">
          <div className="flex min-w-0 items-center gap-1.5">{leftControlsSlot}</div>
          <div className="flex shrink-0 items-center gap-1.5">
            {rightControlsSlot}
            {showStop ? (
              <button
                type="button"
                onClick={onInterrupt}
                title="Stop generating (Esc)"
                aria-label="Stop generating"
                className="orbit-press inline-flex h-[26px] w-[26px] cursor-pointer items-center justify-center rounded-lg bg-rose-500/90 text-white transition-opacity hover:opacity-90"
              >
                <Square className="h-[10.5px] w-[10.5px] fill-current" />
              </button>
            ) : (
              <button
                type="button"
                disabled={!canSend}
                onClick={() => {
                  if (!canSend) return;
                  setHistoryIdx(-1);
                  onSubmit();
                }}
                aria-label="Send message"
                className={cn(
                  "orbit-press inline-flex h-[26px] w-[26px] shrink-0 cursor-pointer items-center justify-center rounded-lg transition-colors",
                  canSend
                    ? "bg-foreground text-background hover:opacity-90"
                    : "bg-muted text-muted-foreground cursor-default opacity-60",
                )}
              >
                <ArrowUp className="h-[14px] w-[14px] stroke-[2.2]" />
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
});
