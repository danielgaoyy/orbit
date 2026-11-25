const HIGHLIGHT_ALL_NAME = "orbit-find-match";
const HIGHLIGHT_ACTIVE_NAME = "orbit-find-active";
const SKIP_ANCESTOR_SELECTOR =
  'button, [role="button"], kbd, [aria-hidden="true"], [data-orbit-find-ignore="true"]';
const MAX_DOM_HIGHLIGHT_RANGES = 250;

interface HighlightRegistryLike {
  set(name: string, highlight: unknown): void;
  delete(name: string): boolean;
}

function getCssHighlights(): HighlightRegistryLike | null {
  if (typeof globalThis === "undefined") return null;
  const cssObj = (globalThis as { CSS?: { highlights?: HighlightRegistryLike } }).CSS;
  const HighlightCtor = (globalThis as { Highlight?: new (...ranges: Range[]) => unknown }).Highlight;
  if (!cssObj?.highlights || typeof HighlightCtor !== "function") {
    return null;
  }
  return cssObj.highlights;
}

function collectTextMatchRanges(
  root: HTMLElement,
  needleLower: string,
  activeTurnEl: HTMLElement | null = null,
): { allRanges: Range[]; activeTurnRanges: Range[] } {
  if (!needleLower || typeof document === "undefined") {
    return { allRanges: [], activeTurnRanges: [] };
  }
  const allRanges: Range[] = [];
  const activeTurnRanges: Range[] = [];
  // Filter at the element level so skipped subtrees (`button`, `kbd`, `aria-hidden`) are
  // pruned in O(1) without calling `parent.closest(...)` on every single text node.
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT, {
    acceptNode(node) {
      if (node.nodeType === Node.ELEMENT_NODE) {
        return (node as Element).matches(SKIP_ANCESTOR_SELECTOR)
          ? NodeFilter.FILTER_REJECT
          : NodeFilter.FILTER_SKIP;
      }
      return node.nodeValue && node.nodeValue.length > 0
        ? NodeFilter.FILTER_ACCEPT
        : NodeFilter.FILTER_SKIP;
    },
  });

  let current = walker.nextNode();
  while (current && allRanges.length < MAX_DOM_HIGHLIGHT_RANGES) {
    const text = current.nodeValue ?? "";
    const lower = text.toLowerCase();
    let offset = 0;
    let inActiveTurn: boolean | null = null;
    while (allRanges.length < MAX_DOM_HIGHLIGHT_RANGES) {
      const idx = lower.indexOf(needleLower, offset);
      if (idx < 0) break;
      try {
        const range = document.createRange();
        range.setStart(current, idx);
        range.setEnd(current, idx + needleLower.length);
        allRanges.push(range);
        if (activeTurnEl) {
          if (inActiveTurn === null) {
            inActiveTurn = activeTurnEl.contains(current);
          }
          if (inActiveTurn) {
            activeTurnRanges.push(range);
          }
        }
      } catch {
        // Ignore stale text nodes mid-mutation
      }
      offset = idx + needleLower.length;
    }
    current = walker.nextNode();
  }
  return { allRanges, activeTurnRanges };
}

export function clearStageFindHighlights(): void {
  const registry = getCssHighlights();
  if (!registry) return;
  registry.delete(HIGHLIGHT_ALL_NAME);
  registry.delete(HIGHLIGHT_ACTIVE_NAME);
}

/**
 * Paint zero-DOM-mutation CSS Custom Highlights across visible mounted turns in `stageContentEl`,
 * emphasizing the active occurrence inside `activeTurnIndex` and returning its `Range` so callers
 * can smoothly scroll tall turns directly to the matched line.
 */
export function applyStageFindHighlights(
  stageContentEl: HTMLElement | null,
  rawQuery: string,
  activeTurnIndex: number | null,
  activeVisibleOccurrenceInTurn: number | null,
): Range | null {
  const needle = rawQuery.trim().toLowerCase();
  if (!stageContentEl || !needle) {
    clearStageFindHighlights();
    return null;
  }

  const turnEl =
    activeTurnIndex !== null && activeTurnIndex >= 0
      ? stageContentEl.querySelector<HTMLElement>(`[data-orbit-turn-index="${activeTurnIndex}"]`)
      : null;

  const { allRanges, activeTurnRanges } = collectTextMatchRanges(stageContentEl, needle, turnEl);
  let activeRange: Range | null = null;

  if (turnEl) {
    const turnRanges =
      activeTurnRanges.length > 0 || allRanges.length < MAX_DOM_HIGHLIGHT_RANGES
        ? activeTurnRanges
        : collectTextMatchRanges(turnEl, needle).allRanges;
    if (turnRanges.length > 0) {
      const clampedIdx =
        activeVisibleOccurrenceInTurn !== null && activeVisibleOccurrenceInTurn >= 0
          ? Math.min(activeVisibleOccurrenceInTurn, turnRanges.length - 1)
          : 0;
      activeRange = turnRanges[clampedIdx] ?? turnRanges[0] ?? null;
    }
  }

  const registry = getCssHighlights();
  if (registry) {
    const HighlightCtor = (globalThis as unknown as { Highlight: new (...ranges: Range[]) => unknown })
      .Highlight;
    if (allRanges.length > 0) {
      registry.set(HIGHLIGHT_ALL_NAME, new HighlightCtor(...allRanges));
    } else {
      registry.delete(HIGHLIGHT_ALL_NAME);
    }
    if (activeRange) {
      registry.set(HIGHLIGHT_ACTIVE_NAME, new HighlightCtor(activeRange));
    } else {
      registry.delete(HIGHLIGHT_ACTIVE_NAME);
    }
  }

  return activeRange;
}

/**
 * Scroll `scrollViewport` smoothly to center `range` when it sits outside the comfortably visible
 * middle band of the viewport (essential for long multi-screen assistant replies).
 */
export function scrollRangeIntoViewport(
  scrollViewport: HTMLElement | null,
  range: Range | null,
  behavior: ScrollBehavior = "smooth",
): void {
  if (!scrollViewport || !range) return;
  const rect = range.getBoundingClientRect();
  if (rect.width === 0 && rect.height === 0) return;

  const vpRect = scrollViewport.getBoundingClientRect();
  const topMargin = 64;
  const bottomMargin = 72;
  const visibleTop = vpRect.top + topMargin;
  const visibleBottom = vpRect.bottom - bottomMargin;
  if (rect.top >= visibleTop && rect.bottom <= visibleBottom) {
    return;
  }

  const delta = rect.top - vpRect.top - vpRect.height * 0.38;
  const targetTop = Math.max(0, scrollViewport.scrollTop + delta);
  scrollViewport.scrollTo({ top: targetTop, behavior });
}
