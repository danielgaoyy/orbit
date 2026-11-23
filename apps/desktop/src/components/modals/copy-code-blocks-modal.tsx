import { useCallback, useEffect, useRef, useState } from "react";
import { Code2, Copy } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog-tabs";
import { ScrollArea } from "@/components/ui/scroll-area";
import type { ExtractedCodeBlock } from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

export function CopyCodeBlocksModal() {
  const target = useOrbitSelector((s) => s.copyCodeBlocksTarget);
  const open = Boolean(target);
  const blocks = target?.blocks ?? [];

  const [activeIndex, setActiveIndex] = useState(0);
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const listRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (open) {
      setActiveIndex(0);
      requestAnimationFrame(() => {
        const firstOption = listRef.current?.querySelector<HTMLButtonElement>(
          '[data-code-block-idx="0"]',
        );
        firstOption?.focus();
      });
    }
  }, [open, target?.agentId, blocks.length]);

  useEffect(() => {
    if (!open) return;
    const container = viewportRef.current;
    if (!container) return;
    const el = container.querySelector<HTMLElement>(`[data-code-block-idx="${activeIndex}"]`);
    el?.scrollIntoView({ block: "nearest" });
  }, [open, activeIndex]);

  const copyBlock = useCallback(async (block: ExtractedCodeBlock) => {
    try {
      await navigator.clipboard.writeText(block.code);
      const langLabel = block.language && block.language !== "text" ? `${block.language} ` : "";
      orbitActions.showToast(
        `Copied ${langLabel}code (${block.line_count} ${block.line_count === 1 ? "line" : "lines"})`,
      );
      orbitActions.closeCopyCodeBlocksModal();
    } catch {
      orbitActions.showToast("Failed to copy code block");
    }
  }, []);

  const focusOption = useCallback((idx: number) => {
    setActiveIndex(idx);
    const el = listRef.current?.querySelector<HTMLButtonElement>(`[data-code-block-idx="${idx}"]`);
    el?.focus();
  }, []);

  const handleDialogKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (blocks.length === 0) return;

    // Never hijack Enter/Space when the user has tabbed to the dialog Close button
    const targetEl = e.target as HTMLElement | null;
    const insideListbox = Boolean(targetEl && listRef.current?.contains(targetEl));

    if (e.key === "ArrowDown" || (e.key === "j" && !e.metaKey && !e.ctrlKey && !e.altKey)) {
      e.preventDefault();
      focusOption((activeIndex + 1) % blocks.length);
      return;
    }
    if (e.key === "ArrowUp" || (e.key === "k" && !e.metaKey && !e.ctrlKey && !e.altKey)) {
      e.preventDefault();
      focusOption((activeIndex - 1 + blocks.length) % blocks.length);
      return;
    }
    if (e.key === "Home" && insideListbox) {
      e.preventDefault();
      focusOption(0);
      return;
    }
    if (e.key === "End" && insideListbox) {
      e.preventDefault();
      focusOption(blocks.length - 1);
      return;
    }
    if (e.key === "Enter" && insideListbox) {
      e.preventDefault();
      const chosen = blocks[Math.min(activeIndex, blocks.length - 1)];
      if (chosen) {
        void copyBlock(chosen);
      }
      return;
    }
    if (!e.metaKey && !e.ctrlKey && !e.altKey && /^[1-9]$/.test(e.key)) {
      const idx = Number.parseInt(e.key, 10) - 1;
      if (idx < blocks.length) {
        e.preventDefault();
        void copyBlock(blocks[idx]);
      }
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen) orbitActions.closeCopyCodeBlocksModal();
      }}
    >
      <DialogContent className="max-w-[520px] gap-3 p-4" onKeyDown={handleDialogKeyDown}>
        <DialogHeader className="gap-0.5 px-1 pr-6">
          <DialogTitle className="flex items-center gap-2 text-[14.5px]">
            <Code2 className="text-muted-foreground size-4 stroke-[1.75]" />
            <span>Copy Code Block</span>
          </DialogTitle>
          <DialogDescription className="text-[12px]">
            {blocks.length} {blocks.length === 1 ? "block" : "blocks"} · most recent first
          </DialogDescription>
        </DialogHeader>

        <ScrollArea viewportRef={viewportRef} viewportClassName="max-h-[340px]">
          <div
            ref={listRef}
            role="listbox"
            aria-label="Code blocks"
            className="space-y-1 pr-1"
          >
            {blocks.map((block, idx) => {
              const active = idx === activeIndex;
              return (
                <button
                  key={`${block.turn_index}-${block.index}`}
                  type="button"
                  role="option"
                  aria-selected={active}
                  tabIndex={active ? 0 : -1}
                  data-code-block-idx={idx}
                  onMouseEnter={() => setActiveIndex(idx)}
                  onFocus={() => setActiveIndex(idx)}
                  onClick={() => void copyBlock(block)}
                  className={cn(
                    "group focus-visible:ring-foreground/25 flex w-full cursor-pointer items-center justify-between gap-3 rounded-[9px] px-2.5 py-2 text-left transition-colors outline-none focus-visible:ring-1",
                    active
                      ? "bg-accent text-accent-foreground"
                      : "text-foreground/90 hover:bg-accent/55",
                  )}
                >
                  <div className="min-w-0 flex-1 space-y-1">
                    <div className="flex items-center gap-2">
                      <span className="bg-muted text-foreground/85 inline-flex items-center rounded-[5px] px-1.5 py-0.5 font-mono text-[11px] font-medium">
                        {block.language || "text"}
                      </span>
                      <span className="text-muted-foreground text-[11.5px] tabular-nums">
                        {block.line_count} {block.line_count === 1 ? "line" : "lines"}
                      </span>
                      <span className="text-muted-foreground/65 text-[11px] tabular-nums">
                        Turn {block.turn_index}
                      </span>
                    </div>
                    <div className="text-muted-foreground truncate font-mono text-[12px]">
                      {block.preview}
                    </div>
                  </div>
                  <Copy
                    className={cn(
                      "size-3.5 shrink-0 transition-opacity",
                      active
                        ? "text-foreground opacity-90"
                        : "text-muted-foreground opacity-0 group-hover:opacity-75",
                    )}
                  />
                </button>
              );
            })}
          </div>
        </ScrollArea>

        <div className="text-muted-foreground/75 flex items-center justify-between border-t border-black/[0.06] px-1 pt-2 text-[11.5px] dark:border-white/[0.07]">
          <span>↑↓ navigate · ↵ copy · 1–9 quick pick</span>
          <span>esc close</span>
        </div>
      </DialogContent>
    </Dialog>
  );
}
