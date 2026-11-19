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
      case "table":
        for (const h of b.headers) {
          total += countSpansLength(h.spans);
        }
        for (const r of b.rows) {
          for (const c of r.cells) {
            total += countSpansLength(c.spans);
          }
        }
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
      case "table": {
        const nextHeaders = block.headers.map((h) => {
          if (rem <= 0) return { spans: [] };
          const len = countSpansLength(h.spans);
          if (len <= rem) {
            rem -= len;
            return h;
          }
          const sliced = sliceSpans(h.spans, rem);
          rem = 0;
          return { spans: sliced };
        });
        const nextRows = [];
        for (const row of block.rows) {
          if (rem <= 0) break;
          const nextCells = row.cells.map((c) => {
            if (rem <= 0) return { spans: [] };
            const len = countSpansLength(c.spans);
            if (len <= rem) {
              rem -= len;
              return c;
            }
            const sliced = sliceSpans(c.spans, rem);
            rem = 0;
            return { spans: sliced };
          });
          nextRows.push({ cells: nextCells });
        }
        out.push({
          ...block,
          headers: nextHeaders,
          rows: nextRows,
        });
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
  const revealedRef = useRef<number>(revealedChars);
  const rafIdRef = useRef<number>(0);
  const prevFrameMsRef = useRef<number | null>(null);

  targetRef.current = targetChars;

  useEffect(() => {
    if (!streaming) {
      if (rafIdRef.current !== 0) {
        window.cancelAnimationFrame(rafIdRef.current);
        rafIdRef.current = 0;
      }
      prevFrameMsRef.current = null;
      revealedRef.current = targetChars;
      setRevealedChars((prev) => (prev === targetChars ? prev : targetChars));
      return;
    }

    if (targetChars < revealedRef.current) {
      revealedRef.current = targetChars;
      setRevealedChars(targetChars);
    }

    if (revealedRef.current >= targetChars || rafIdRef.current !== 0) {
      return;
    }

    prevFrameMsRef.current = null;

    const stepFrame = (nowMs: number) => {
      const prevMs = prevFrameMsRef.current;
      const elapsed =
        prevMs === null ? FRAME_INTERVAL_MS : Math.min(MAX_ELAPSED_MS, nowMs - prevMs);

      if (elapsed >= FRAME_INTERVAL_MS * 0.85) {
        prevFrameMsRef.current = nowMs;
        const cur = revealedRef.current;
        const target = targetRef.current;
        const backlog = target - cur;
        if (backlog > 0) {
          const delta = Math.max(1, Math.ceil((backlog * elapsed) / TEXT_REVEAL_HORIZON_MS));
          const next = Math.min(target, cur + delta);
          revealedRef.current = next;
          setRevealedChars(next);
        }
      }

      if (revealedRef.current < targetRef.current) {
        rafIdRef.current = window.requestAnimationFrame(stepFrame);
      } else {
        rafIdRef.current = 0;
        prevFrameMsRef.current = null;
      }
    };

    rafIdRef.current = window.requestAnimationFrame(stepFrame);
  }, [streaming, targetChars]);

  useEffect(() => {
    return () => {
      if (rafIdRef.current !== 0) {
        window.cancelAnimationFrame(rafIdRef.current);
        rafIdRef.current = 0;
      }
    };
  }, []);

  return useMemo(
    () => (streaming && revealedChars < targetChars ? sliceRichBlocks(blocks, revealedChars) : blocks),
    [blocks, streaming, revealedChars, targetChars],
  );
}
