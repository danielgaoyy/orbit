import { memo, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { ArrowUp } from "lucide-react";
import { SimpleTooltip } from "@/components/ui/overlays";
import { useI18n } from "@/lib/i18n";
import { useImeCompositionGuard } from "@/lib/ime";
import { modSymbol } from "@/lib/keybindings";
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
  rightControlsSlot?: ReactNode | ((widthPx: number) => ReactNode);
  className?: string;
}

/**
 * Reusable conversation composer matching Orbit's primary `OmniComposer` surface, typography,
 * deterministic off-flow div mirror auto-height (handles both typing and large multi-line pastes),
 * keyboard semantics (`enterToSend`, `Cmd/Ctrl+Enter`, prompt history recall), and Stop control.
 */
export const ConversationComposer = memo(function ConversationComposer({
  value,
  onChange,
  onSubmit,
  onInterrupt,
  isBusy = false,
  enterToSend = true,
  placeholder,
  focusNonce,
  promptHistory,
  variant = "docked",
  topSlot,
  leftControlsSlot,
  rightControlsSlot,
  className,
}: ConversationComposerProps) {
  const { t } = useI18n();
  const effectivePlaceholder = placeholder ?? t("composer.conversationFallbackPlaceholder");
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const mirrorRef = useRef<HTMLDivElement | null>(null);
  const lastEscTimeRef = useRef<number>(0);
  const [historyIdx, setHistoryIdx] = useState<number>(-1);
  const [widthTick, setWidthTick] = useState(0);
  const [composerWidthPx, setComposerWidthPx] = useState(0);
  const savedDraftRef = useRef<string>("");
  const ime = useImeCompositionGuard();

  const minHeight = variant === "compact" ? 38 : 46;
  const maxHeight = variant === "compact" ? 180 : 220;

  useEffect(() => {
    if (focusNonce === undefined) return;
    const raf = requestAnimationFrame(() => textareaRef.current?.focus());
    return () => cancelAnimationFrame(raf);
  }, [focusNonce]);

  useEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    let lastW = el.clientWidth;
    setComposerWidthPx(lastW);
    const ro = new ResizeObserver((entries) => {
      const nextW = Math.round(entries[0]?.contentRect.width ?? 0);
      if (nextW > 0 && nextW !== lastW) {
        lastW = nextW;
        setComposerWidthPx(nextW);
        setWidthTick((n) => n + 1);
      }
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  useLayoutEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    if (!value) {
      el.style.height = `${minHeight}px`;
      el.style.overflowY = "hidden";
      return;
    }
    const mirror = mirrorRef.current;
    if (!mirror) return;
    const measured = Math.max(mirror.offsetHeight, mirror.scrollHeight);
    const nextH = Math.min(maxHeight, Math.max(minHeight, measured));
    el.style.height = `${nextH}px`;
    el.style.overflowY = measured > maxHeight ? "auto" : "hidden";
  }, [value, minHeight, maxHeight, widthTick]);

  const canSend = value.trim().length > 0 && !isBusy;
  const showStop = isBusy && Boolean(onInterrupt) && value.trim().length === 0;

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (ime.shouldIgnoreKeyDown(e)) return;

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
      const now = Date.now();
      if (now - lastEscTimeRef.current < 420) {
        lastEscTimeRef.current = 0;
        e.preventDefault();
        onInterrupt();
      } else {
        lastEscTimeRef.current = now;
      }
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

  const textStyleClass =
    variant === "compact"
      ? "px-[12px] py-[9px] text-[13px] leading-[20px]"
      : "px-[14px] py-[12px] text-[15px] leading-[22px]";

  const resolvedRightControls =
    typeof rightControlsSlot === "function"
      ? rightControlsSlot(composerWidthPx)
      : rightControlsSlot;

  return (
    <div
      className={cn(
        "bg-surface-base shrink-0",
        variant === "compact" ? "px-3 pt-1 pb-3" : "px-3 pt-0 pb-4",
        className,
      )}
    >
      {topSlot}
      <div
        className={cn(
          "composer-surface flex flex-col",
          variant === "compact" ? "rounded-[16px]" : "rounded-[20px]",
        )}
      >
        <div className="relative">
          {/* Off-flow div mirror sizer for deterministic multi-line & paste expansion */}
          <div
            ref={mirrorRef}
            aria-hidden="true"
            className={cn(
              "pointer-events-none invisible absolute top-0 right-0 left-0 -z-10 block w-full whitespace-pre-wrap break-words",
              textStyleClass,
            )}
          >
            {value}
            {"\u200b"}
          </div>

          <textarea
            ref={textareaRef}
            rows={1}
            value={value}
            onChange={(e) => {
              if (historyIdx !== -1) setHistoryIdx(-1);
              onChange(e.target.value);
            }}
            {...ime.compositionProps}
            onKeyDown={handleKeyDown}
            placeholder={effectivePlaceholder}
            style={{ minHeight: `${minHeight}px`, maxHeight: `${maxHeight}px` }}
            className={cn(
              "text-ink-primary placeholder:text-ink-faint block w-full resize-none overflow-y-hidden bg-transparent outline-none",
              textStyleClass,
            )}
          />
        </div>

        {/* Bottom Control Bar matching primary OmniComposer rhythm (`34px` controls, `pb-2`) */}
        <div className="flex items-center justify-between gap-2 px-2.5 pt-0 pb-2">
          <div className="flex min-w-0 flex-1 items-center gap-0.5 overflow-hidden">
            {leftControlsSlot}
          </div>
          <div className="flex shrink-0 items-center gap-1.5">
            {resolvedRightControls}
            {showStop ? (
              <SimpleTooltip label={t("composer.stopGeneration")} shortcut="Esc Esc" side="top">
                <button
                  type="button"
                  onClick={onInterrupt}
                  aria-label={t("composer.stopGeneration")}
                  className={cn(
                    "orbit-press bg-surface-recessed hover:bg-accent inline-flex shrink-0 cursor-pointer items-center justify-center rounded-full",
                    variant === "compact" ? "size-[30px]" : "size-8.5",
                  )}
                >
                  <span className="bg-signal-danger size-[11px] rounded-[2.5px]" />
                </button>
              </SimpleTooltip>
            ) : (
              <SimpleTooltip
                label={t("common.send")}
                shortcut={enterToSend ? "↵" : `${modSymbol()}↵`}
                side="top"
              >
                <button
                  type="button"
                  disabled={!canSend}
                  onClick={() => {
                    if (!canSend) return;
                    setHistoryIdx(-1);
                    onSubmit();
                  }}
                  aria-label={t("common.send")}
                  className={cn(
                    "orbit-press inline-flex shrink-0 cursor-pointer items-center justify-center rounded-full",
                    variant === "compact" ? "size-[30px]" : "size-8.5",
                    canSend
                      ? "bg-action-primary-bg text-action-primary-fg hover:bg-action-primary-hover"
                      : "text-ink-muted bg-surface-recessed pointer-events-none opacity-65",
                  )}
                >
                  <ArrowUp
                    className={
                      variant === "compact"
                        ? "h-[15px] w-[15px] stroke-[2.2]"
                        : "size-[17px] stroke-[2.15]"
                    }
                  />
                </button>
              </SimpleTooltip>
            )}
          </div>
        </div>
      </div>
    </div>
  );
});
