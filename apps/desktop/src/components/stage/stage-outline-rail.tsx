import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useI18n } from "@/lib/i18n";
import { isImeKeyEvent } from "@/lib/ime";
import {
  isActiveInEarlier,
  outlinePromptsEqual,
  partitionOutlinePrompts,
  type OutlinePromptItem,
} from "@/lib/outline-rail";
import { cn } from "@/lib/utils";

export type { OutlinePromptItem };

type JumpHandler = (seq: number) => void;

const EarlierRow = memo(function EarlierRow({
  item,
  number,
  active,
  jumping,
  onSelect,
}: {
  item: OutlinePromptItem;
  number: number;
  active: boolean;
  jumping: boolean;
  onSelect: JumpHandler;
}) {
  const row = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (active) row.current?.scrollIntoView({ block: "nearest" });
  }, [active]);

  return (
    <Button
      ref={row}
      variant="ghost"
      size="sm"
      type="button"
      title={item.preview}
      aria-current={active ? "step" : undefined}
      aria-busy={jumping || undefined}
      onClick={() => onSelect(item.seq)}
      className="w-full justify-start gap-2 rounded-lg px-2 text-left text-[13px] font-normal text-ink-secondary hover:bg-surface-recessed aria-[current=step]:bg-surface-recessed aria-[current=step]:font-medium aria-[current=step]:text-ink-primary aria-busy:bg-surface-recessed motion-safe:aria-busy:animate-pulse"
    >
      <span className="w-5 shrink-0 text-right font-mono text-[11px] text-ink-faint tabular-nums">
        {number}
      </span>
      <span className="min-w-0 truncate">{item.preview}</span>
    </Button>
  );
});

function EarlierRows({
  earlier,
  activeSeq,
  jumpingToSeq,
  onSelect,
}: {
  earlier: readonly OutlinePromptItem[];
  activeSeq: number | null;
  jumpingToSeq: number | null;
  onSelect: JumpHandler;
}) {
  return earlier.map((item, idx) => (
    <EarlierRow
      key={item.seq}
      item={item}
      number={idx + 1}
      active={item.seq === activeSeq}
      jumping={item.seq === jumpingToSeq}
      onSelect={onSelect}
    />
  ));
}

const EarlierCluster = memo(function EarlierCluster({
  earlier,
  activeSeq,
  jumpingToSeq,
  open,
  onOpenChange,
  onJumpToSeq,
}: {
  earlier: readonly OutlinePromptItem[];
  activeSeq: number | null;
  jumpingToSeq: number | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onJumpToSeq: JumpHandler;
}) {
  const { t } = useI18n();
  const trigger = useRef<HTMLButtonElement>(null);
  const select = useCallback(
    (seq: number) => {
      trigger.current?.focus({ preventScroll: true });
      onOpenChange(false);
      onJumpToSeq(seq);
    },
    [onOpenChange, onJumpToSeq],
  );

  return (
    <Popover open={open} onOpenChange={onOpenChange}>
      <PopoverTrigger
        ref={trigger}
        openOnHover
        delay={0}
        closeDelay={160}
        aria-label={t("stage.earlierPromptCount", { count: earlier.length })}
        aria-busy={jumpingToSeq !== null || undefined}
        className={cn(
          "group/cluster mb-1 flex h-[22px] w-full shrink-0 cursor-pointer flex-col items-start justify-center gap-[2.5px] rounded-sm pl-[1.5px] focus-visible:outline-1 focus-visible:outline-ink-secondary",
          jumpingToSeq !== null && "motion-safe:animate-pulse",
        )}
      >
        {[0, 1, 2].map((dot) => (
          <span
            key={dot}
            className={cn(
              "h-[2.5px] w-[7px] origin-left rounded-full transition-[scale,background-color] duration-150 motion-reduce:transition-none",
              open
                ? "bg-ink-primary"
                : activeSeq !== null || jumpingToSeq !== null
                  ? "scale-x-75 bg-ink-secondary"
                  : "scale-x-[0.36] bg-ink-faint group-hover/cluster:bg-ink-secondary",
            )}
          />
        ))}
      </PopoverTrigger>
      <PopoverContent
        side="right"
        align="center"
        sideOffset={4}
        collisionPadding={8}
        aria-label={t("stage.earlierPrompts")}
        className="flex max-h-[min(260px,var(--available-height))] w-[min(264px,var(--available-width))] flex-col overflow-hidden rounded-xl border-border-subtle! bg-surface-overlay p-0"
      >
        <div className="flex shrink-0 items-center justify-between gap-2 border-b border-border-subtle px-3 py-1.5">
          <span className="text-[11px] font-medium text-ink-muted">
            {t("stage.earlierPrompts")}
          </span>
          <span className="font-mono text-[11px] text-ink-faint tabular-nums">
            1–{earlier.length}
          </span>
        </div>
        <ScrollArea
          className="flex min-h-0 flex-col"
          viewportClassName="min-h-0 overscroll-contain px-1.5 py-1"
        >
          <EarlierRows
            earlier={earlier}
            activeSeq={activeSeq}
            jumpingToSeq={jumpingToSeq}
            onSelect={select}
          />
        </ScrollArea>
      </PopoverContent>
    </Popover>
  );
});

const OutlineTickItem = memo(function OutlineTickItem({
  seq,
  preview,
  index,
  number,
  active,
  jumping,
  onPreview,
  onJumpToSeq,
}: {
  seq: number;
  preview: string;
  index: number;
  number: number;
  active: boolean;
  jumping: boolean;
  onPreview: (index: number | null) => void;
  onJumpToSeq: JumpHandler;
}) {
  const { t } = useI18n();
  return (
    <button
      type="button"
      aria-label={t("stage.jumpToPrompt", { num: number, preview })}
      aria-current={active ? "step" : undefined}
      aria-busy={jumping || undefined}
      onMouseEnter={() => onPreview(index)}
      onFocus={() => onPreview(index)}
      onClick={() => onJumpToSeq(seq)}
      className="group/tick flex h-[14px] min-h-0 w-full cursor-pointer items-center rounded-sm focus-visible:outline-1 focus-visible:outline-ink-secondary [&:has(+button:hover)>span]:scale-x-75 [button:hover+&>span]:scale-x-75"
    >
      <span
        className={cn(
          "h-[3px] w-4 origin-left rounded-full transition-[scale,background-color] duration-150 ease-out group-hover/tick:scale-100 group-hover/tick:bg-ink-primary group-focus-visible/tick:scale-100 group-focus-visible/tick:bg-ink-primary motion-reduce:transition-none",
          active || jumping
            ? "scale-x-[0.7] scale-y-[0.67] bg-ink-secondary"
            : "scale-x-[0.375] scale-y-[0.67] bg-ink-primary/25",
          jumping && "motion-safe:animate-pulse",
        )}
      />
    </button>
  );
});

const RecentTicks = memo(function RecentTicks({
  recent,
  firstNumber,
  activeSeq,
  jumpingToSeq,
  previewEnabled,
  onJumpToSeq,
}: {
  recent: readonly OutlinePromptItem[];
  firstNumber: number;
  activeSeq: number | null;
  jumpingToSeq: number | null;
  previewEnabled: boolean;
  onJumpToSeq: JumpHandler;
}) {
  const [hoveredTickIdx, setHoveredTickIdx] = useState<number | null>(null);
  // Escape dismisses a mouse preview even while keyboard focus stays in the composer.
  useEffect(() => {
    if (!previewEnabled) {
      setHoveredTickIdx(null);
      return;
    }
    const dismiss = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !isImeKeyEvent(e)) setHoveredTickIdx(null);
    };
    window.addEventListener("keydown", dismiss);
    return () => window.removeEventListener("keydown", dismiss);
  }, [previewEnabled]);

  const item = previewEnabled && hoveredTickIdx !== null ? recent[hoveredTickIdx] : undefined;
  const isPreviewOpen = Boolean(item && hoveredTickIdx !== null);
  const lastPreviewRef = useRef<{ idx: number; text: string } | null>(null);
  const wasPreviewOpenRef = useRef(false);

  if (item && hoveredTickIdx !== null) {
    lastPreviewRef.current = { idx: hoveredTickIdx, text: item.preview };
  }

  const activePreviewIdx =
    isPreviewOpen && hoveredTickIdx !== null
      ? hoveredTickIdx
      : Math.min(lastPreviewRef.current?.idx ?? 0, Math.max(0, recent.length - 1));
  const previewText = item?.preview ?? lastPreviewRef.current?.text ?? "";
  const glidePreviewY = isPreviewOpen && wasPreviewOpenRef.current;

  useEffect(() => {
    wasPreviewOpenRef.current = isPreviewOpen;
  }, [isPreviewOpen]);

  return (
    <div
      className="relative flex min-h-0 w-full flex-col"
      onMouseLeave={() => setHoveredTickIdx(null)}
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget)) setHoveredTickIdx(null);
      }}
    >
      {recent.map((p, i) => (
        <OutlineTickItem
          key={p.seq}
          seq={p.seq}
          preview={p.preview}
          index={i}
          number={firstNumber + i}
          active={p.seq === activeSeq}
          jumping={p.seq === jumpingToSeq}
          onPreview={setHoveredTickIdx}
          onJumpToSeq={onJumpToSeq}
        />
      ))}
      <div
        aria-hidden="true"
        className={cn(
          "pointer-events-none absolute inset-y-0 left-full z-30 w-56",
          glidePreviewY &&
            "motion-safe:transition-transform motion-safe:duration-100 motion-safe:ease-[var(--ease-out-expo)]",
        )}
        style={{
          transform: `translate3d(0, ${(100 * (activePreviewIdx + 0.5)) / recent.length}%, 0)`,
        }}
      >
        <div
          className={cn(
            "floating-surface origin-left -translate-y-1/2 rounded-xl border border-border-subtle! bg-surface-overlay px-3 py-2 text-[13px] leading-snug text-ink-primary transition-[opacity,scale] motion-reduce:transition-none",
            isPreviewOpen
              ? "scale-100 opacity-100 duration-[135ms] ease-[var(--ease-out-expo)]"
              : "scale-[0.97] opacity-0 duration-[95ms] ease-[var(--ease-exit)]",
          )}
        >
          <div className="line-clamp-2 break-words">{previewText}</div>
        </div>
      </div>
    </div>
  );
});

export const ChatOutlineRail = memo(
  function ChatOutlineRail({
    prompts,
    activeSeq,
    jumpingToSeq = null,
    onJumpToSeq,
    visible = true,
    offsetPx = 0,
    offsetTransition = "none",
    isAnimating = false,
  }: {
    prompts: readonly OutlinePromptItem[];
    activeSeq: number | null;
    jumpingToSeq?: number | null;
    onJumpToSeq: JumpHandler;
    visible?: boolean;
    offsetPx?: number;
    offsetTransition?: string;
    isAnimating?: boolean;
  }) {
    const { t } = useI18n();
    const { earlier, recent } = useMemo(() => partitionOutlinePrompts(prompts), [prompts]);
    const [clusterOpen, setClusterOpen] = useState(false);
    useEffect(() => {
      if (!visible || !earlier.length) setClusterOpen(false);
    }, [visible, earlier.length]);
    if (!recent.length) return null;
    return (
      <div
        style={{
          transform: offsetPx ? `translate3d(${offsetPx}px, 0, 0)` : undefined,
          transition: offsetTransition,
          willChange: isAnimating ? "transform" : undefined,
        }}
        className="pointer-events-none absolute top-[10%] bottom-[12%] left-[6px] z-30 flex w-6 items-center"
      >
        <nav
          aria-label={t("stage.outline")}
          aria-hidden={!visible}
          inert={!visible}
          className={cn(
            "flex max-h-full w-full flex-col items-start transition-[opacity,transform,visibility] duration-200 ease-[var(--ease-panel)] motion-reduce:transition-none",
            visible
              ? "pointer-events-auto visible opacity-100"
              : "pointer-events-none invisible -translate-x-1.5 opacity-0",
          )}
        >
          {earlier.length > 0 && (
            <EarlierCluster
              earlier={earlier}
              activeSeq={isActiveInEarlier(earlier, activeSeq) ? activeSeq : null}
              jumpingToSeq={isActiveInEarlier(earlier, jumpingToSeq) ? jumpingToSeq : null}
              open={visible && clusterOpen}
              onOpenChange={setClusterOpen}
              onJumpToSeq={onJumpToSeq}
            />
          )}
          <RecentTicks
            recent={recent}
            firstNumber={earlier.length + 1}
            activeSeq={activeSeq}
            jumpingToSeq={jumpingToSeq}
            previewEnabled={visible && !clusterOpen}
            onJumpToSeq={onJumpToSeq}
          />
        </nav>
      </div>
    );
  },
  (prev, next) =>
    prev.activeSeq === next.activeSeq &&
    prev.jumpingToSeq === next.jumpingToSeq &&
    prev.onJumpToSeq === next.onJumpToSeq &&
    prev.visible === next.visible &&
    prev.offsetPx === next.offsetPx &&
    prev.offsetTransition === next.offsetTransition &&
    prev.isAnimating === next.isAnimating &&
    outlinePromptsEqual(prev.prompts, next.prompts),
);
