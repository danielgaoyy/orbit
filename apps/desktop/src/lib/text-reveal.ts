import { useEffect, useMemo, useRef, useState } from "react";
import type { InlineSpanView, RichBlockView } from "@/lib/orbit-ipc";

const TEXT_REVEAL_HORIZON_MS = 150;
const FRAME_INTERVAL_MS = 1000 / 60;
const MAX_ELAPSED_MS = 240;

const graphemeSegmenter =
  typeof Intl !== "undefined" && typeof Intl.Segmenter === "function"
    ? new Intl.Segmenter(undefined, { granularity: "grapheme" })
    : null;

function clampToSafeGraphemeBoundary(text: string, index: number): number {
  if (index <= 0) return 0;
  if (index >= text.length) return text.length;
  if (graphemeSegmenter) {
    const seg = graphemeSegmenter.segment(text).containing(index);
    return seg?.index ?? index;
  }
  const prevCode = text.charCodeAt(index - 1);
  if (prevCode >= 0xd800 && prevCode <= 0xdbff) {
    return Math.max(0, index - 1);
  }
  return index;
}

function countSpansLength(spans: InlineSpanView[]): number {
  let len = 0;
  for (const s of spans) len += s.text.length;
  return len;
}

export function countRichBlocksChars(blocks: RichBlockView[]): number {
  let total = 0;
  for (const b of blocks) {
    switch (b.kind) {
      case "heading":
      case "paragraph":
      case "list_item":
      case "block_quote":
        total += countSpansLength(b.spans);
        break;
      case "code_block":
        total += b.raw_code.length;
        break;
      case "horizontal_rule":
        total += 1;
        break;
    }
  }
  return total;
}

function sliceSpans(spans: InlineSpanView[], budget: number): InlineSpanView[] {
  if (budget <= 0) return [];
  const out: InlineSpanView[] = [];
  let rem = budget;
  for (const span of spans) {
    if (rem <= 0) break;
    if (span.text.length <= rem) {
      out.push(span);
      rem -= span.text.length;
    } else {
      const cut = clampToSafeGraphemeBoundary(span.text, rem);
      if (cut > 0) {
        out.push({ ...span, text: span.text.slice(0, cut) });
      }
      rem = 0;
    }
  }
  return out;
}

export function sliceRichBlocks(blocks: RichBlockView[], budget: number): RichBlockView[] {
  const total = countRichBlocksChars(blocks);
  if (budget >= total) return blocks;
  if (budget <= 0) return [];

  const out: RichBlockView[] = [];
  let rem = budget;

  for (const block of blocks) {
    if (rem <= 0) break;
    switch (block.kind) {
      case "heading":
      case "paragraph":
      case "list_item":
      case "block_quote": {
        const len = countSpansLength(block.spans);
        if (len <= rem) {
          out.push(block);
          rem -= len;
        } else {
          const sliced = sliceSpans(block.spans, rem);
          if (sliced.length > 0) {
            out.push({ ...block, spans: sliced });
          }
          rem = 0;
        }
        break;
      }
      case "code_block": {
        const len = block.raw_code.length;
        if (len <= rem) {
          out.push(block);
          rem -= len;
        } else {
          const cut = clampToSafeGraphemeBoundary(block.raw_code, rem);
          const partialCode = block.raw_code.slice(0, cut);
          const lineCount = partialCode.split("\n").length;
          out.push({
            ...block,
            raw_code: partialCode,
            lines: block.lines.slice(0, lineCount),
          });
          rem = 0;
        }
        break;
      }
      case "horizontal_rule":
        out.push(block);
        rem -= 1;
        break;
    }
  }

  return out;
}

/**
 * Paces lumpy network/daemon streaming deltas over a smooth 150ms 60Hz horizon
 * while rendering historical or already-completed messages 100% immediately on first paint.
 */
export function usePacedRichBlocks(blocks: RichBlockView[], streaming = false): RichBlockView[] {
  const targetChars = useMemo(() => countRichBlocksChars(blocks), [blocks]);
  const [revealedChars, setRevealedChars] = useState<number>(() => targetChars);
  const targetRef = useRef<number>(targetChars);
  targetRef.current = targetChars;

  useEffect(() => {
    if (!streaming) {
      setRevealedChars(targetChars);
      return;
    }
    setRevealedChars((prev) => (targetChars < prev ? targetChars : prev));
  }, [streaming, targetChars]);

  useEffect(() => {
    if (!streaming || revealedChars >= targetChars) return;

    let rafId = 0;
    let prevFrameMs: number | null = null;

    const stepFrame = (nowMs: number) => {
      const elapsed =
        prevFrameMs === null ? FRAME_INTERVAL_MS : Math.min(MAX_ELAPSED_MS, nowMs - prevFrameMs);
      if (elapsed >= FRAME_INTERVAL_MS) {
        prevFrameMs = nowMs - (elapsed % FRAME_INTERVAL_MS);
        setRevealedChars((cur) => {
          const target = targetRef.current;
          const backlog = target - cur;
          if (backlog <= 0) return target;
          const delta = Math.max(1, Math.ceil((backlog * elapsed) / TEXT_REVEAL_HORIZON_MS));
          return Math.min(target, cur + delta);
        });
      }
      if (targetRef.current > revealedChars) {
        rafId = window.requestAnimationFrame(stepFrame);
      }
    };

    rafId = window.requestAnimationFrame(stepFrame);
    return () => window.cancelAnimationFrame(rafId);
  }, [streaming, targetChars, revealedChars]);

  return useMemo(
    () => (streaming && revealedChars < targetChars ? sliceRichBlocks(blocks, revealedChars) : blocks),
    [blocks, streaming, revealedChars, targetChars],
  );
}
