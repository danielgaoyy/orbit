import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  STAGE_FIND_REMOTE_DEBOUNCE_MS,
  STAGE_FIND_REMOTE_LIMIT,
  doesMatchRequireOlderHistoryPagination,
  mergeLoadedAndRemoteStageMatches,
  resolveNextActiveFindKey,
  stepStageFindIndex,
  type StageFindMatch,
} from "@/components/stage/stage-find-helpers";
import {
  applyStageFindHighlights,
  clearStageFindHighlights,
  scrollRangeIntoViewport,
} from "@/components/stage/stage-find-highlight";
import {
  type ConversationSearchHit,
  type StageTurnView,
  orbitIpc,
} from "@/lib/orbit-ipc";

const EMPTY_FIND_HITS: readonly ConversationSearchHit[] = Object.freeze([]);
const EMPTY_FIND_MATCHES: readonly StageFindMatch[] = Object.freeze([]);
const EMPTY_MATCHED_TURN_KEYS: ReadonlySet<string> = new Set();

export interface StageFindJumpArgs {
  index: number;
  align: "start" | "center";
  behavior: "auto" | "smooth";
}

export interface UseStageFindOptions {
  selectedHostId: string | null;
  selectedAgentId: string | null;
  activeSessionScope: string | null;
  isDraft: boolean;
  stageFindFocusNonce: number;
  stageTurns: StageTurnView[];
  oldestLoadedSeq: number;
  windowStartTurnIndex: number;
  hasOlderHistory: boolean;
  stageContainerRef: React.RefObject<HTMLDivElement | null>;
  scrollRef: React.RefObject<HTMLDivElement | null>;
  contentRef: React.RefObject<HTMLDivElement | null>;
  jumpToTurnRef: React.RefObject<((jump: StageFindJumpArgs) => void) | null>;
  activeSessionScopeRef: React.RefObject<string | null>;
  interruptFollow: () => void;
  syncScrollMetrics: () => void;
  paginateUntilSeqLoaded: (
    hostId: string,
    agentId: string,
    scope: string,
    seq: number,
    isCancelled: () => boolean,
  ) => Promise<{ index: number; exact: boolean }>;
}

export interface StageFindController {
  open: boolean;
  query: string;
  activeIndex: number;
  totalMatches: number;
  activeTurnKey: string | null;
  matchedTurnKeys: ReadonlySet<string>;
  hasUnloadedMatches: boolean;
  isActiveMatchUnloaded: boolean;
  isRemoteSearching: boolean;
  isPaginatingJump: boolean;
  onQueryChange: (nextQuery: string) => void;
  onStep: (delta: 1 | -1) => void;
  onClose: () => void;
  repaintHighlights: () => void;
  resetForSessionChange: () => void;
}

/**
 * Cohesive in-session Find controller for `StreamStage`:
 * - Instant client-side substring scanning over loaded turns
 * - Debounced (180ms) remote daemon `SearchMessages(agent_id)` full-history merge
 * - Zero-DOM-mutation CSS Custom Highlight API painting + tall-turn viewport scrolling
 * - Automatic multi-page `loadEarlierStageTurns` pagination when jumping to unloaded history
 */
export function useStageFind({
  selectedHostId,
  selectedAgentId,
  activeSessionScope,
  isDraft,
  stageFindFocusNonce,
  stageTurns,
  oldestLoadedSeq,
  windowStartTurnIndex,
  hasOlderHistory,
  stageContainerRef,
  scrollRef,
  contentRef,
  jumpToTurnRef,
  activeSessionScopeRef,
  interruptFollow,
  syncScrollMetrics,
  paginateUntilSeqLoaded,
}: UseStageFindOptions): StageFindController {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [remoteResult, setRemoteResult] = useState<{
    scope: string;
    query: string;
    hits: ConversationSearchHit[];
  }>({ scope: "", query: "", hits: [] });
  const [remoteLoading, setRemoteLoading] = useState(false);
  const [activeKey, setActiveKey] = useState<string | null>(null);
  const [paginatingSeq, setPaginatingSeq] = useState<number | null>(null);

  const lastFocusNonceRef = useRef(stageFindFocusNonce);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const remoteReqIdRef = useRef(0);
  const jumpReqIdRef = useRef(0);
  const highlightRafRef = useRef<number | null>(null);
  const navSignatureRef = useRef<string | null>(null);
  const highlightStateRef = useRef<{
    open: boolean;
    query: string;
    activeTurnIndex: number | null;
    activeVisibleOccurrence: number | null;
  }>({
    open: false,
    query: "",
    activeTurnIndex: null,
    activeVisibleOccurrence: null,
  });

  // Open / refocus floating `StageFindBar` when `⌘F` / `Ctrl+F` triggers in Stage
  useEffect(() => {
    if (stageFindFocusNonce === lastFocusNonceRef.current) return;
    lastFocusNonceRef.current = stageFindFocusNonce;
    if (!selectedHostId || !selectedAgentId || isDraft) return;
    const active = document.activeElement as HTMLElement | null;
    if (active && !active.closest('[data-orbit-stage-find="true"]')) {
      previousFocusRef.current = active;
    }
    setOpen(true);
  }, [stageFindFocusNonce, selectedHostId, selectedAgentId, isDraft]);

  // Debounced (180ms) remote daemon `SearchMessages(agent_id)` query for unloaded history
  useEffect(() => {
    if (!open || !selectedHostId || !selectedAgentId || !activeSessionScope) {
      setRemoteLoading(false);
      return;
    }
    const trimmed = query.trim();
    const reqId = (remoteReqIdRef.current += 1);
    if (!trimmed) {
      setRemoteResult({ scope: activeSessionScope, query: "", hits: [] });
      setRemoteLoading(false);
      return;
    }

    let cancelled = false;
    setRemoteLoading(true);
    const timer = window.setTimeout(() => {
      void orbitIpc
        .searchMessagesRemote({
          query: trimmed,
          hostId: selectedHostId,
          agentId: selectedAgentId,
          limit: STAGE_FIND_REMOTE_LIMIT,
        })
        .then((res) => {
          if (cancelled || remoteReqIdRef.current !== reqId) return;
          setRemoteResult({
            scope: activeSessionScope,
            query: trimmed,
            hits: res.hits ?? [],
          });
          setRemoteLoading(false);
        })
        .catch(() => {
          if (cancelled || remoteReqIdRef.current !== reqId) return;
          setRemoteResult({ scope: activeSessionScope, query: trimmed, hits: [] });
          setRemoteLoading(false);
        });
    }, STAGE_FIND_REMOTE_DEBOUNCE_MS);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [open, query, selectedHostId, selectedAgentId, activeSessionScope]);

  const activeRemoteHits = useMemo(() => {
    const trimmed = query.trim();
    if (!open || !trimmed || !activeSessionScope) return EMPTY_FIND_HITS;
    if (remoteResult.scope === activeSessionScope && remoteResult.query === trimmed) {
      return remoteResult.hits;
    }
    return EMPTY_FIND_HITS;
  }, [open, query, activeSessionScope, remoteResult]);

  const matches = useMemo<readonly StageFindMatch[]>(() => {
    if (!open) return EMPTY_FIND_MATCHES;
    return mergeLoadedAndRemoteStageMatches(stageTurns, query, activeRemoteHits, oldestLoadedSeq);
  }, [open, stageTurns, query, activeRemoteHits, oldestLoadedSeq]);

  const effectiveActiveKey = useMemo(
    () => resolveNextActiveFindKey(matches, activeKey),
    [matches, activeKey],
  );

  const activeIndex = useMemo(() => {
    if (!effectiveActiveKey) return -1;
    return matches.findIndex((m) => m.key === effectiveActiveKey);
  }, [matches, effectiveActiveKey]);

  const activeMatch = activeIndex >= 0 ? matches[activeIndex] : null;
  const hasUnloadedMatches = useMemo(() => matches.some((m) => m.unloaded), [matches]);

  highlightStateRef.current = {
    open,
    query,
    activeTurnIndex: activeMatch?.turnIndex ?? null,
    activeVisibleOccurrence: activeMatch?.visibleOccurrenceInTurn ?? null,
  };

  const onQueryChange = useCallback((nextQuery: string) => {
    navSignatureRef.current = null;
    setQuery(nextQuery);
    setActiveKey(null);
  }, []);

  const onStep = useCallback(
    (delta: 1 | -1) => {
      const nextIdx = stepStageFindIndex(activeIndex, matches.length, delta);
      if (nextIdx < 0) return;
      navSignatureRef.current = null;
      setActiveKey(matches[nextIdx].key);
    },
    [activeIndex, matches],
  );

  const onClose = useCallback(() => {
    jumpReqIdRef.current += 1;
    remoteReqIdRef.current += 1;
    navSignatureRef.current = null;
    setOpen(false);
    setRemoteLoading(false);
    setPaginatingSeq(null);
    clearStageFindHighlights();
    const prev = previousFocusRef.current;
    previousFocusRef.current = null;
    window.requestAnimationFrame(() => {
      if (prev && prev.isConnected) {
        prev.focus({ preventScroll: true });
      } else {
        stageContainerRef.current?.focus({ preventScroll: true });
      }
    });
  }, [stageContainerRef]);

  const repaintHighlights = useCallback(() => {
    const fh = highlightStateRef.current;
    if (fh.open && fh.query.trim()) {
      applyStageFindHighlights(
        contentRef.current,
        fh.query,
        fh.activeTurnIndex,
        fh.activeVisibleOccurrence,
      );
    }
  }, [contentRef]);

  const resetForSessionChange = useCallback(() => {
    jumpReqIdRef.current += 1;
    remoteReqIdRef.current += 1;
    navSignatureRef.current = null;
    setOpen(false);
    setRemoteLoading(false);
    setPaginatingSeq(null);
    setRemoteResult({ scope: "", query: "", hits: [] });
    setActiveKey(null);
    clearStageFindHighlights();
  }, []);

  // Navigate to active match (auto-paginating older history if unloaded) & paint highlights
  useEffect(() => {
    if (!open) {
      clearStageFindHighlights();
      return;
    }
    const trimmed = query.trim();
    if (!trimmed) {
      clearStageFindHighlights();
      setPaginatingSeq(null);
      return;
    }
    if (!activeMatch || !activeSessionScope) {
      applyStageFindHighlights(contentRef.current, trimmed, null, null);
      return;
    }

    const navSig = `${activeSessionScope}::${trimmed}::${activeMatch.key}`;
    if (navSignatureRef.current === navSig) {
      applyStageFindHighlights(
        contentRef.current,
        trimmed,
        activeMatch.turnIndex,
        activeMatch.visibleOccurrenceInTurn,
      );
      return;
    }

    navSignatureRef.current = navSig;
    const needsPagination = doesMatchRequireOlderHistoryPagination(
      activeMatch,
      stageTurns,
      windowStartTurnIndex,
      hasOlderHistory,
    );

    if (needsPagination && selectedHostId && selectedAgentId) {
      interruptFollow();
      const reqId = (jumpReqIdRef.current += 1);
      const targetSeq = activeMatch.seq;
      setPaginatingSeq(targetSeq);
      const hostId = selectedHostId;
      const agentId = selectedAgentId;
      const scope = activeSessionScope;

      void (async () => {
        try {
          const finalMatch = await paginateUntilSeqLoaded(
            hostId,
            agentId,
            scope,
            targetSeq,
            () => jumpReqIdRef.current !== reqId,
          );
          if (jumpReqIdRef.current !== reqId || activeSessionScopeRef.current !== scope) {
            return;
          }
          if (finalMatch.index >= 0) {
            const idx = finalMatch.index;
            jumpToTurnRef.current?.({ index: idx, align: "center", behavior: "auto" });
            window.requestAnimationFrame(() => {
              if (jumpReqIdRef.current !== reqId) return;
              jumpToTurnRef.current?.({ index: idx, align: "center", behavior: "auto" });
              window.requestAnimationFrame(() => {
                if (jumpReqIdRef.current !== reqId) return;
                const range = applyStageFindHighlights(contentRef.current, trimmed, idx, 0);
                scrollRangeIntoViewport(scrollRef.current, range, "auto");
                syncScrollMetrics();
              });
            });
          }
        } finally {
          if (jumpReqIdRef.current === reqId) {
            setPaginatingSeq(null);
          }
        }
      })();
      return;
    }

    if (activeMatch.turnIndex !== null && activeMatch.turnIndex >= 0) {
      interruptFollow();
      const targetIdx = activeMatch.turnIndex;
      const targetOcc = activeMatch.visibleOccurrenceInTurn;
      jumpToTurnRef.current?.({ index: targetIdx, align: "center", behavior: "smooth" });
      if (highlightRafRef.current !== null) {
        window.cancelAnimationFrame(highlightRafRef.current);
      }
      highlightRafRef.current = window.requestAnimationFrame(() => {
        highlightRafRef.current = null;
        const range = applyStageFindHighlights(
          contentRef.current,
          trimmed,
          targetIdx,
          targetOcc,
        );
        scrollRangeIntoViewport(scrollRef.current, range, "smooth");
      });
    } else {
      applyStageFindHighlights(contentRef.current, trimmed, null, null);
    }
  }, [
    open,
    query,
    activeMatch,
    activeSessionScope,
    stageTurns,
    windowStartTurnIndex,
    hasOlderHistory,
    selectedHostId,
    selectedAgentId,
    contentRef,
    scrollRef,
    jumpToTurnRef,
    activeSessionScopeRef,
    interruptFollow,
    paginateUntilSeqLoaded,
    syncScrollMetrics,
  ]);

  useEffect(() => {
    return () => {
      clearStageFindHighlights();
      if (highlightRafRef.current !== null) {
        window.cancelAnimationFrame(highlightRafRef.current);
      }
    };
  }, []);

  const matchedTurnKeys = useMemo<ReadonlySet<string>>(() => {
    if (!open || matches.length === 0) return EMPTY_MATCHED_TURN_KEYS;
    const set = new Set<string>();
    for (const m of matches) {
      if (m.turnKey) set.add(m.turnKey);
    }
    return set.size > 0 ? set : EMPTY_MATCHED_TURN_KEYS;
  }, [open, matches]);

  return {
    open,
    query,
    activeIndex,
    totalMatches: matches.length,
    activeTurnKey: open && activeMatch?.turnKey ? activeMatch.turnKey : null,
    matchedTurnKeys,
    hasUnloadedMatches,
    isActiveMatchUnloaded: Boolean(activeMatch?.unloaded),
    isRemoteSearching: remoteLoading,
    isPaginatingJump: paginatingSeq !== null,
    onQueryChange,
    onStep,
    onClose,
    repaintHighlights,
    resetForSessionChange,
  };
}
