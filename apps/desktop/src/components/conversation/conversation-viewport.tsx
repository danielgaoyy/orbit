import { memo, useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { ArrowDown, Check, ChevronDown, ChevronUp, Copy } from "lucide-react";
import { MarkdownLinkProvider } from "@/components/markdown-blocks";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn } from "@/lib/utils";

/**
 * Shared sticky-bottom conversation viewport used across side panels (`/btw` Side Question,
 * Subagent inspection, and secondary threads). Unlocks sticky scrolling immediately on user
 * upward scroll and reveals a restrained jump-to-latest pill.
 */
export const ConversationViewport = memo(function ConversationViewport({
  hostId,
  cwd,
  contentVersion,
  resetKey,
  className,
  viewportClassName,
  contentClassName,
  children,
}: {
  hostId?: string | null;
  cwd?: string | null;
  contentVersion: string | number;
  resetKey?: string;
  className?: string;
  viewportClassName?: string;
  contentClassName?: string;
  children: ReactNode;
}) {
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const stickToBottomRef = useRef(true);
  const [showJumpLatest, setShowJumpLatest] = useState(false);

  const checkNearBottom = useCallback((el: HTMLDivElement): boolean => {
    const gap = el.scrollHeight - el.scrollTop - el.clientHeight;
    return gap <= 48;
  }, []);

  const scrollToLatest = useCallback((smooth = false) => {
    const el = viewportRef.current;
    if (!el) return;
    stickToBottomRef.current = true;
    setShowJumpLatest(false);
    el.scrollTo({
      top: el.scrollHeight,
      behavior: smooth ? "smooth" : "auto",
    });
  }, []);

  useEffect(() => {
    stickToBottomRef.current = true;
    setShowJumpLatest(false);
    requestAnimationFrame(() => {
      const el = viewportRef.current;
      if (el) el.scrollTop = el.scrollHeight;
    });
  }, [resetKey]);

  useEffect(() => {
    if (!stickToBottomRef.current) return;
    const el = viewportRef.current;
    if (!el) return;
    const raf = requestAnimationFrame(() => {
      if (stickToBottomRef.current && viewportRef.current) {
        viewportRef.current.scrollTop = viewportRef.current.scrollHeight;
      }
    });
    return () => cancelAnimationFrame(raf);
  }, [contentVersion]);

  const handleScroll = useCallback(() => {
    const el = viewportRef.current;
    if (!el) return;
    const nearBottom = checkNearBottom(el);
    stickToBottomRef.current = nearBottom;
    setShowJumpLatest(!nearBottom && el.scrollHeight > el.clientHeight + 80);
  }, [checkNearBottom]);

  const handleWheel = useCallback((e: React.WheelEvent<HTMLDivElement>) => {
    if (e.deltaY < -2) {
      stickToBottomRef.current = false;
    }
  }, []);

  return (
    <div className={cn("bg-canvas relative flex min-h-0 flex-1 flex-col overflow-hidden", className)}>
      <MarkdownLinkProvider hostId={hostId ?? undefined} cwd={cwd ?? undefined}>
        <ScrollArea
          className="min-h-0 flex-1"
          viewportRef={viewportRef}
          viewportClassName={cn("px-4 py-4", viewportClassName)}
          contentClassName={contentClassName}
          onViewportScroll={handleScroll}
          onWheel={handleWheel}
        >
          {children}
        </ScrollArea>
      </MarkdownLinkProvider>

      {showJumpLatest && (
        <div className="pointer-events-none absolute inset-x-0 bottom-3 z-10 flex justify-center">
          <button
            type="button"
            onClick={() => scrollToLatest(true)}
            className="pointer-events-auto border-border bg-elevated/95 text-foreground hover:bg-accent inline-flex cursor-pointer items-center gap-1.5 rounded-full border px-3 py-1 text-[11.5px] font-medium shadow-md backdrop-blur-sm transition-colors"
          >
            <ArrowDown className="h-[12px] w-[12px]" />
            <span>Jump to latest</span>
          </button>
        </div>
      )}
    </div>
  );
});

/**
 * Shared user message bubble matching the main conversation aesthetic with
 * automatic collapse/expand for long pasted prompts.
 */
export const ConversationUserBubble = memo(function ConversationUserBubble({
  text,
  compact = false,
}: {
  text: string;
  compact?: boolean;
}) {
  const lineCount = text.split("\n").length;
  const isLong = text.length > 360 || lineCount > 7;
  const [expanded, setExpanded] = useState(false);

  return (
    <div className="flex justify-end">
      <div
        className={cn(
          "prompt-bubble-surface selectable-text text-foreground max-w-[90%] rounded-2xl rounded-tr-md leading-relaxed whitespace-pre-wrap break-words",
          compact ? "px-3.5 py-2 text-[12.5px]" : "px-4 py-2.5 text-[13.5px]",
        )}
      >
        <div className={cn(isLong && !expanded && "line-clamp-6")}>{text}</div>
        {isLong && (
          <button
            type="button"
            onClick={() => setExpanded((v) => !v)}
            className="text-muted-foreground hover:text-foreground mt-1 inline-flex cursor-pointer items-center gap-1 text-[11px] font-medium"
          >
            {expanded ? (
              <>
                <ChevronUp className="h-3 w-3" />
                <span>Show less</span>
              </>
            ) : (
              <>
                <ChevronDown className="h-3 w-3" />
                <span>Show full message</span>
              </>
            )}
          </button>
        )}
      </div>
    </div>
  );
});

/**
 * Shared assistant message footer with one-click copy, metadata label, and optional trailing action.
 */
export const ConversationMessageFooter = memo(function ConversationMessageFooter({
  copyText,
  metaLabel,
  rightSlot,
}: {
  copyText: string;
  metaLabel?: string;
  rightSlot?: ReactNode;
}) {
  const [copied, setCopied] = useState(false);

  const handleCopy = () => {
    void navigator.clipboard.writeText(copyText).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1400);
    });
  };

  return (
    <div className="flex items-center justify-between gap-2 pt-0.5">
      <div className="flex min-w-0 items-center gap-1.5">
        <button
          type="button"
          title="Copy response"
          onClick={handleCopy}
          className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded-md transition-colors"
        >
          {copied ? (
            <Check className="h-[12px] w-[12px] text-emerald-500" />
          ) : (
            <Copy className="h-[12px] w-[12px]" />
          )}
        </button>
        {metaLabel && (
          <span className="text-muted-foreground/75 truncate text-[11px]">{metaLabel}</span>
        )}
      </div>
      {rightSlot && <div className="flex shrink-0 items-center gap-1">{rightSlot}</div>}
    </div>
  );
});
