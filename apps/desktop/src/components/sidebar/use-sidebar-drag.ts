import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { flushSync } from "react-dom";
import {
  DRAG_THRESHOLD_SORT_PX,
  type InlineSortSlot,
  buildInlineSortSlots,
  clampInlineSortDelta,
  computeInlineSortNeighborOffset,
  computeInlineSortSettleDelta,
  computeInlineSortTargetIndex,
  isNoOpLinearDrop,
  prefersReducedMotion,
  startTransformSettle,
} from "@/lib/drag/drag-constants";
import { useEdgeAutoScroll } from "@/lib/drag/use-edge-auto-scroll";
import { usePointerDrag } from "@/lib/drag/use-pointer-drag";
import type { SidebarProjectGroupView } from "@/lib/orbit-ipc";
import { projectPathScopeKey } from "@/lib/projects";
import type { SidebarProjectSort } from "@/state/store-types";
import { orbitActions } from "@/state/orbit-store";

export interface ThreadDragSource {
  hostId: string;
  projectId: string;
  agentId: string;
  title: string;
  scopeKey: string;
  workspaceId: string | null;
}

export interface PinnedDragSource {
  threadKey: string;
  title: string;
}

/** Only used for cross-group / cross-scope thread insertion; same-scope reorder uses live neighbor displacement. */
export interface ThreadDropIndicator {
  scopeKey: string;
  targetAgentId: string;
  position: "before" | "after";
}

export interface SidebarInlineSortState {
  phase: "dragging" | "settling";
  fromId: string;
  fromIndex: number;
  overIndex: number;
  ids: readonly string[];
  slots: readonly InlineSortSlot[];
}

export interface ThreadInlineSortState extends SidebarInlineSortState {
  scopeKey: string;
  projectId: string;
}

export interface LiftedThreadRowMeta {
  label: string;
  width: number;
  height: number;
  settling: boolean;
}

interface ThreadRowEntry {
  el: HTMLDivElement;
  hostId: string;
  projectId: string;
  agentId: string;
  scopeKey: string;
  workspaceId: string | null;
}

interface WorkspaceHeaderEntry {
  el: HTMLDivElement;
  hostId: string;
  projectId: string;
  workspaceId: string;
}

interface InboxAreaEntry {
  el: HTMLDivElement;
  hostId: string;
  projectId: string;
  scopeKey: string;
}

type CrossScopeThreadTarget =
  | {
      kind: "thread";
      scopeKey: string;
      workspaceId: string | null;
      targetAgentId: string;
      position: "before" | "after";
    }
  | { kind: "group"; workspaceId: string }
  | { kind: "inbox"; projectId: string; scopeKey: string }
  | null;

const PROJECT_SORT_MAX_LEAD_IN_PX = 56;

export function getInlineSortNeighborOffset(
  state: SidebarInlineSortState | null,
  itemId: string,
): number {
  if (!state || state.fromId === itemId) return 0;
  const idx = state.ids.indexOf(itemId);
  if (idx < 0) return 0;
  return computeInlineSortNeighborOffset(state.slots, state.fromIndex, state.overIndex, idx);
}

/**
 * Unified tactile "drag-along" pointer engine for Conversations, Projects, and Pinned threads
 * in `FleetSidebar`.
 *
 * Key engineering invariants:
 * - Zero React re-renders per pixel: dragged items track the pointer 1:1 via direct DOM `translate3d`.
 * - Real-time sibling displacement (`150ms --ease-out-expo`) for Pinned threads, Projects
 *   (variable-height), and same-scope Conversations — no static insertion line for in-list reorders.
 * - Conversations use a portaled full-width lifted row (`DragRowPreview`) so dragging out of
 *   overflow-hidden Curated Groups stays 100% unclipped while cross-group targets light up cleanly.
 * - Smooth `140ms --ease-out-expo` settle into the vacated target slot before committing reorder,
 *   with `useLayoutEffect` transform cleanup to guarantee zero reverse-transition flicker.
 * - Full macOS WKWebView & touch/keyboard safety via shared `usePointerDrag` + `useEdgeAutoScroll`.
 */
export function useSidebarDrag(params: {
  displayProjects: ReadonlyArray<{ project: SidebarProjectGroupView }>;
  pinnedThreadKeys?: readonly string[];
  sidebarProjectSort: SidebarProjectSort;
  scrollViewportRef: React.RefObject<HTMLDivElement | null>;
}) {
  const { displayProjects, pinnedThreadKeys = [], sidebarProjectSort, scrollViewportRef } = params;

  const displayProjectsRef = useRef(displayProjects);
  displayProjectsRef.current = displayProjects;
  const pinnedThreadKeysRef = useRef(pinnedThreadKeys);
  pinnedThreadKeysRef.current = pinnedThreadKeys;
  const sortModeRef = useRef(sidebarProjectSort);
  sortModeRef.current = sidebarProjectSort;

  const [liftedThread, setLiftedThread] = useState<LiftedThreadRowMeta | null>(null);
  const [draggingThread, setDraggingThread] = useState<ThreadDragSource | null>(null);
  const [threadInlineSort, setThreadInlineSort] = useState<ThreadInlineSortState | null>(null);
  const [threadDropIndicator, setThreadDropIndicator] = useState<ThreadDropIndicator | null>(null);
  const [dragOverDropId, setDragOverDropId] = useState<string | null>(null);
  const [projectSortState, setProjectSortState] = useState<SidebarInlineSortState | null>(null);
  const [pinnedSortState, setPinnedSortState] = useState<SidebarInlineSortState | null>(null);

  const liftedThreadElRef = useRef<HTMLDivElement | null>(null);
  const liftedThreadPosRef = useRef<{ x: number; y: number; scale: number }>({
    x: 0,
    y: 0,
    scale: 1,
  });
  const draggedProjectElRef = useRef<HTMLDivElement | null>(null);
  const draggedPinnedElRef = useRef<HTMLDivElement | null>(null);
  const activeSettleFlushRef = useRef<(() => void) | null>(null);
  const settleCancelRef = useRef<(() => void) | null>(null);
  const activeScrollTickRef = useRef<((clientX: number, clientY: number) => void) | null>(null);

  const projectRowRefs = useRef<Map<string, HTMLDivElement>>(new Map());
  const threadRowRefs = useRef<Map<string, ThreadRowEntry>>(new Map());
  const pinnedRowRefs = useRef<Map<string, HTMLDivElement>>(new Map());
  const workspaceHeaderRefs = useRef<Map<string, WorkspaceHeaderEntry>>(new Map());
  const inboxAreaRefs = useRef<Map<string, InboxAreaEntry>>(new Map());
  const scopeAgentIdsRef = useRef<Map<string, string[]>>(new Map());

  const {
    isDragging: isPointerDragging,
    start: startPointerDrag,
    shouldSuppressClick,
  } = usePointerDrag();

  const handleAutoScrollTick = useCallback((clientX: number, clientY: number) => {
    activeScrollTickRef.current?.(clientX, clientY);
  }, []);

  const { update: updateAutoScroll, stop: stopAutoScroll } = useEdgeAutoScroll({
    viewportRef: scrollViewportRef,
    axis: "y",
    onScrollTick: handleAutoScrollTick,
  });

  const clearSettle = useCallback(() => {
    settleCancelRef.current?.();
    settleCancelRef.current = null;
    activeSettleFlushRef.current = null;
  }, []);

  // Unmount cleanup for any pending settle animation
  useEffect(() => {
    return () => {
      settleCancelRef.current?.();
      settleCancelRef.current = null;
      activeSettleFlushRef.current = null;
      if (draggedProjectElRef.current) {
        draggedProjectElRef.current.style.transition = "";
        draggedProjectElRef.current.style.transform = "";
        draggedProjectElRef.current = null;
      }
      if (draggedPinnedElRef.current) {
        draggedPinnedElRef.current.style.transition = "";
        draggedPinnedElRef.current.style.transform = "";
        draggedPinnedElRef.current = null;
      }
    };
  }, []);

  // Clear residual direct DOM transition + transform inside the same `flushSync` commit as `null` sort state
  useLayoutEffect(() => {
    if (projectSortState !== null) return;
    if (draggedProjectElRef.current) {
      draggedProjectElRef.current.style.transition = "";
      draggedProjectElRef.current.style.transform = "";
      draggedProjectElRef.current = null;
    }
  }, [projectSortState]);

  useLayoutEffect(() => {
    if (pinnedSortState !== null) return;
    if (draggedPinnedElRef.current) {
      draggedPinnedElRef.current.style.transition = "";
      draggedPinnedElRef.current.style.transform = "";
      draggedPinnedElRef.current = null;
    }
  }, [pinnedSortState]);

  // Position portaled lifted thread row once on mount before first paint (never overwriting settle animation)
  const hasLiftedThread = liftedThread !== null;
  useLayoutEffect(() => {
    const el = liftedThreadElRef.current;
    if (!hasLiftedThread || !el) return;
    const { x, y, scale } = liftedThreadPosRef.current;
    el.style.transform = `translate3d(${x}px, ${y}px, 0) scale(${scale})`;
  }, [hasLiftedThread]);

  const syncScopeAgentIds = useCallback((nextScopes: ReadonlyMap<string, string[]>) => {
    const map = scopeAgentIdsRef.current;
    map.clear();
    for (const [k, v] of nextScopes.entries()) {
      map.set(k, v);
    }
  }, []);

  const registerProjectRow = useCallback((projectId: string, el: HTMLDivElement | null) => {
    if (el) projectRowRefs.current.set(projectId, el);
    else projectRowRefs.current.delete(projectId);
  }, []);

  const registerThreadRow = useCallback(
    (
      refKey: string,
      entry: Omit<ThreadRowEntry, "el"> | null,
      el: HTMLDivElement | null,
    ) => {
      if (el && entry) {
        threadRowRefs.current.set(refKey, { ...entry, el });
      } else {
        const existing = threadRowRefs.current.get(refKey);
        if (!entry || (existing && existing.scopeKey === entry.scopeKey)) {
          threadRowRefs.current.delete(refKey);
        }
      }
    },
    [],
  );

  const registerPinnedRow = useCallback((threadKey: string, el: HTMLDivElement | null) => {
    if (el) pinnedRowRefs.current.set(threadKey, el);
    else pinnedRowRefs.current.delete(threadKey);
  }, []);

  const registerWorkspaceHeader = useCallback(
    (
      workspaceId: string,
      meta: { hostId: string; projectId: string },
      el: HTMLDivElement | null,
    ) => {
      if (el) {
        workspaceHeaderRefs.current.set(workspaceId, {
          el,
          hostId: meta.hostId,
          projectId: meta.projectId,
          workspaceId,
        });
      } else {
        workspaceHeaderRefs.current.delete(workspaceId);
      }
    },
    [],
  );

  const registerInboxArea = useCallback(
    (
      projectId: string,
      meta: { hostId: string; scopeKey: string },
      el: HTMLDivElement | null,
    ) => {
      if (el) {
        inboxAreaRefs.current.set(projectId, {
          el,
          hostId: meta.hostId,
          projectId,
          scopeKey: meta.scopeKey,
        });
      } else {
        inboxAreaRefs.current.delete(projectId);
      }
    },
    [],
  );

  const startThreadDrag = useCallback(
    (e: React.PointerEvent<HTMLDivElement>, source: ThreadDragSource) => {
      activeSettleFlushRef.current?.();

      const sourceRefKey = `${source.hostId}::${source.agentId}`;
      let startX = e.clientX;
      let startY = e.clientY;
      let startScroll = 0;
      let origLeft = 0;
      let origTop = 0;
      let origWidth = 220;
      let origHeight = 30;
      let sameScopeIds: string[] = [];
      let sameScopeSlots: InlineSortSlot[] = [];
      let fromIndex = -1;
      let overIndex = -1;
      let crossTarget: CrossScopeThreadTarget = null;
      let lastCrossIndicatorKey = "";
      let lastDropId: string | null = null;
      let currentLiftedX = 0;
      let currentLiftedY = 0;
      let latestPointerY = e.clientY;

      const applyLiftedTransform = (x: number, y: number, scale: number) => {
        currentLiftedX = x;
        currentLiftedY = y;
        liftedThreadPosRef.current = { x, y, scale };
        const el = liftedThreadElRef.current;
        if (el) {
          el.style.transform = `translate3d(${x}px, ${y}px, 0) scale(${scale})`;
        }
      };

      const updateAtPointer = (clientX: number, clientY: number) => {
        latestPointerY = clientY;
        const dampedDeltaX = Math.max(-14, Math.min(14, (clientX - startX) * 0.18));
        const nextX = Math.max(
          6,
          Math.min(window.innerWidth - origWidth - 6, origLeft + dampedDeltaX),
        );
        const nextY = Math.max(
          6,
          Math.min(window.innerHeight - origHeight - 6, origTop + (clientY - startY)),
        );
        applyLiftedTransform(nextX, nextY, 1);

        // 1. Check cross-group targets first (Curated Group header, different-scope rows, Inbox ungroup)
        let bestCross: CrossScopeThreadTarget = null;

        for (const wsEntry of workspaceHeaderRefs.current.values()) {
          if (
            wsEntry.hostId !== source.hostId ||
            wsEntry.projectId !== source.projectId ||
            wsEntry.workspaceId === source.workspaceId ||
            wsEntry.el.closest('.orbit-collapse-grid[data-open="false"]')
          ) {
            continue;
          }
          const rect = wsEntry.el.getBoundingClientRect();
          if (
            clientX >= rect.left - 8 &&
            clientX <= rect.right + 16 &&
            clientY >= rect.top - 2 &&
            clientY <= rect.bottom + 2
          ) {
            bestCross = { kind: "group", workspaceId: wsEntry.workspaceId };
            break;
          }
        }

        if (!bestCross) {
          for (const entry of threadRowRefs.current.values()) {
            if (
              entry.hostId !== source.hostId ||
              entry.projectId !== source.projectId ||
              entry.scopeKey === source.scopeKey ||
              entry.el.closest('.orbit-collapse-grid[data-open="false"]')
            ) {
              continue;
            }
            const rect = entry.el.getBoundingClientRect();
            if (rect.height === 0) continue;
            if (
              clientX >= rect.left - 12 &&
              clientX <= rect.right + 24 &&
              clientY >= rect.top - 2 &&
              clientY <= rect.bottom + 2
            ) {
              const position = clientY < rect.top + rect.height / 2 ? "before" : "after";
              bestCross = {
                kind: "thread",
                scopeKey: entry.scopeKey,
                workspaceId: entry.workspaceId,
                targetAgentId: entry.agentId,
                position,
              };
              break;
            }
          }
        }

        if (!bestCross && source.workspaceId !== null) {
          for (const inboxEntry of inboxAreaRefs.current.values()) {
            if (
              inboxEntry.hostId !== source.hostId ||
              inboxEntry.projectId !== source.projectId ||
              inboxEntry.el.closest('.orbit-collapse-grid[data-open="false"]')
            ) {
              continue;
            }
            const rect = inboxEntry.el.getBoundingClientRect();
            if (
              clientY >= rect.top - 4 &&
              clientY <= rect.bottom + 12 &&
              clientX <= rect.right + 24
            ) {
              bestCross = {
                kind: "inbox",
                projectId: inboxEntry.projectId,
                scopeKey: inboxEntry.scopeKey,
              };
              break;
            }
          }
        }

        crossTarget = bestCross;

        // 2. Update same-scope live neighbor displacement (silently glides back while hovering cross-group)
        if (fromIndex >= 0 && sameScopeSlots.length >= 2) {
          let nextOver = fromIndex;
          if (!bestCross) {
            const currentScroll = scrollViewportRef.current?.scrollTop ?? startScroll;
            const rawDelta = clientY - startY + (currentScroll - startScroll);
            const clamped = clampInlineSortDelta(sameScopeSlots, fromIndex, rawDelta);
            nextOver = computeInlineSortTargetIndex(sameScopeSlots, fromIndex, clamped);
          }
          if (nextOver !== overIndex) {
            overIndex = nextOver;
            setThreadInlineSort({
              phase: "dragging",
              scopeKey: source.scopeKey,
              projectId: source.projectId,
              fromId: source.agentId,
              fromIndex,
              overIndex: nextOver,
              ids: sameScopeIds,
              slots: sameScopeSlots,
            });
          }
        }

        // 3. Only show DropIndicatorLine / container ring for cross-scope targets
        const nextIndicatorKey =
          bestCross?.kind === "thread"
            ? `${bestCross.scopeKey}::${bestCross.targetAgentId}::${bestCross.position}`
            : "";
        const nextDropId =
          bestCross?.kind === "group"
            ? `ws:${bestCross.workspaceId}`
            : bestCross?.kind === "inbox"
              ? `inbox:${bestCross.projectId}`
              : null;

        if (nextIndicatorKey !== lastCrossIndicatorKey) {
          lastCrossIndicatorKey = nextIndicatorKey;
          setThreadDropIndicator(
            bestCross?.kind === "thread"
              ? {
                  scopeKey: bestCross.scopeKey,
                  targetAgentId: bestCross.targetAgentId,
                  position: bestCross.position,
                }
              : null,
          );
        }
        if (nextDropId !== lastDropId) {
          lastDropId = nextDropId;
          setDragOverDropId(nextDropId);
        }
      };

      const clearThreadVisuals = () => {
        setDraggingThread(null);
        setThreadInlineSort(null);
        setThreadDropIndicator(null);
        setDragOverDropId(null);
        setLiftedThread(null);
      };

      const startSettle = (targetOverIdx: number, shouldCommitSameScope: boolean) => {
        const scrollDiff = (scrollViewportRef.current?.scrollTop ?? startScroll) - startScroll;
        const targetDelta =
          fromIndex >= 0 && sameScopeSlots.length >= 2
            ? computeInlineSortSettleDelta(sameScopeSlots, fromIndex, targetOverIdx)
            : 0;
        const targetY = origTop + targetDelta - scrollDiff;

        let finished = false;
        const finalize = () => {
          if (finished) return;
          finished = true;
          clearSettle();
          flushSync(() => {
            if (
              shouldCommitSameScope &&
              fromIndex >= 0 &&
              targetOverIdx >= 0 &&
              targetOverIdx !== fromIndex
            ) {
              const targetAgentId = sameScopeIds[targetOverIdx];
              const position: "before" | "after" = targetOverIdx > fromIndex ? "after" : "before";
              const fullIds = scopeAgentIdsRef.current.get(source.scopeKey) ?? sameScopeIds;
              if (
                targetAgentId &&
                !isNoOpLinearDrop(fullIds, source.agentId, targetAgentId, position)
              ) {
                const orderedIds = fullIds.filter((id) => id !== source.agentId);
                let toIdx = orderedIds.indexOf(targetAgentId);
                if (toIdx >= 0) {
                  if (position === "after") toIdx += 1;
                  orderedIds.splice(toIdx, 0, source.agentId);
                } else {
                  orderedIds.unshift(source.agentId);
                }
                orbitActions.reorderThreads(source.scopeKey, orderedIds);
              }
            }
            clearThreadVisuals();
          });
        };

        const reduced = prefersReducedMotion();
        const dist = Math.hypot(currentLiftedX - origLeft, currentLiftedY - targetY);
        const liftedEl = liftedThreadElRef.current;
        if (reduced || !liftedEl || dist < 1) {
          finalize();
          return;
        }

        clearSettle();
        activeSettleFlushRef.current = finalize;

        if (fromIndex >= 0 && sameScopeSlots.length >= 2) {
          setThreadInlineSort({
            phase: "settling",
            scopeKey: source.scopeKey,
            projectId: source.projectId,
            fromId: source.agentId,
            fromIndex,
            overIndex: targetOverIdx,
            ids: sameScopeIds,
            slots: sameScopeSlots,
          });
        }
        setLiftedThread({
          label: source.title,
          width: origWidth,
          height: origHeight,
          settling: true,
        });
        currentLiftedX = origLeft;
        currentLiftedY = targetY;
        liftedThreadPosRef.current = { x: origLeft, y: targetY, scale: 1 };
        settleCancelRef.current = startTransformSettle(
          liftedEl,
          `translate3d(${origLeft}px, ${targetY}px, 0) scale(1)`,
          finalize,
        );
      };

      startPointerDrag(e, {
        thresholdPx: DRAG_THRESHOLD_SORT_PX,
        cursor: "grabbing",
        ignoreDoubleClick: true,
        ignoreSelector: "[data-thread-action], input",
        onActivate: (frame) => {
          const srcEntry = threadRowRefs.current.get(sourceRefKey);
          if (!srcEntry) return false;
          const srcRect = srcEntry.el.getBoundingClientRect();
          if (srcRect.height === 0) return false;

          startX = frame.startX;
          startY = frame.startY;
          origLeft = srcRect.left;
          origTop = srcRect.top;
          origWidth = srcRect.width;
          origHeight = srcRect.height;

          const viewport = scrollViewportRef.current;
          const vpRect = viewport?.getBoundingClientRect() ?? null;
          startScroll = viewport?.scrollTop ?? 0;

          // Snapshot all visible rows in the same scope sorted visually top-to-bottom
          const mountedSameScope: Array<{ id: string; rect: DOMRect }> = [];
          for (const entry of threadRowRefs.current.values()) {
            if (
              entry.hostId !== source.hostId ||
              entry.projectId !== source.projectId ||
              entry.scopeKey !== source.scopeKey ||
              entry.el.closest('.orbit-collapse-grid[data-open="false"]')
            ) {
              continue;
            }
            const rect = entry.el.getBoundingClientRect();
            if (rect.height === 0) continue;
            mountedSameScope.push({ id: entry.agentId, rect });
          }
          mountedSameScope.sort((a, b) => a.rect.top - b.rect.top);
          sameScopeIds = mountedSameScope.map((m) => m.id);
          fromIndex = sameScopeIds.indexOf(source.agentId);
          overIndex = fromIndex;
          sameScopeSlots = buildInlineSortSlots(
            mountedSameScope.map((m) => ({
              start: vpRect ? m.rect.top - vpRect.top + startScroll : m.rect.top,
              size: m.rect.height,
            })),
          );

          liftedThreadPosRef.current = {
            x: origLeft,
            y: origTop + (frame.clientY - startY),
            scale: 1,
          };
          currentLiftedX = liftedThreadPosRef.current.x;
          currentLiftedY = liftedThreadPosRef.current.y;

          activeScrollTickRef.current = updateAtPointer;
          setDraggingThread(source);
          if (fromIndex >= 0 && sameScopeSlots.length >= 2) {
            setThreadInlineSort({
              phase: "dragging",
              scopeKey: source.scopeKey,
              projectId: source.projectId,
              fromId: source.agentId,
              fromIndex,
              overIndex: fromIndex,
              ids: sameScopeIds,
              slots: sameScopeSlots,
            });
          }
          setLiftedThread({
            label: source.title,
            width: origWidth,
            height: origHeight,
            settling: false,
          });
          return true;
        },
        onMove: (frame) => {
          updateAutoScroll(frame.clientX, frame.clientY);
          updateAtPointer(frame.clientX, frame.clientY);
        },
        onCommit: () => {
          activeScrollTickRef.current = null;
          stopAutoScroll();
          if (crossTarget?.kind === "thread") {
            const target = crossTarget;
            flushSync(() => {
              if (target.workspaceId) {
                void orbitActions.moveAgentToWorkspace(
                  source.hostId,
                  source.agentId,
                  target.workspaceId,
                );
              } else {
                void orbitActions.moveAgentToWorkspace(source.hostId, source.agentId, null, {
                  makeUnclassified: true,
                });
              }
              if (sortModeRef.current === "manual") {
                const fullIds = scopeAgentIdsRef.current.get(target.scopeKey) ?? [];
                const orderedIds = fullIds.filter((id) => id !== source.agentId);
                let toIdx = orderedIds.indexOf(target.targetAgentId);
                if (toIdx >= 0) {
                  if (target.position === "after") toIdx += 1;
                  orderedIds.splice(toIdx, 0, source.agentId);
                } else {
                  orderedIds.unshift(source.agentId);
                }
                orbitActions.reorderThreads(target.scopeKey, orderedIds);
              }
              clearThreadVisuals();
            });
          } else if (crossTarget?.kind === "group") {
            const targetWorkspaceId = crossTarget.workspaceId;
            flushSync(() => {
              void orbitActions.moveAgentToWorkspace(
                source.hostId,
                source.agentId,
                targetWorkspaceId,
              );
              clearThreadVisuals();
            });
          } else if (crossTarget?.kind === "inbox") {
            flushSync(() => {
              void orbitActions.moveAgentToWorkspace(source.hostId, source.agentId, null, {
                makeUnclassified: true,
              });
              clearThreadVisuals();
            });
          } else {
            // Recompute overIndex at final pointer position in case no extra move arrived after scroll
            if (fromIndex >= 0 && sameScopeSlots.length >= 2) {
              const currentScroll = scrollViewportRef.current?.scrollTop ?? startScroll;
              const rawDelta = latestPointerY - startY + (currentScroll - startScroll);
              const clamped = clampInlineSortDelta(sameScopeSlots, fromIndex, rawDelta);
              overIndex = computeInlineSortTargetIndex(sameScopeSlots, fromIndex, clamped);
            }
            startSettle(overIndex >= 0 ? overIndex : fromIndex, overIndex !== fromIndex);
          }
        },
        onCancel: () => {
          activeScrollTickRef.current = null;
          stopAutoScroll();
          setThreadDropIndicator(null);
          setDragOverDropId(null);
          startSettle(fromIndex, false);
        },
      });
    },
    [clearSettle, scrollViewportRef, startPointerDrag, stopAutoScroll, updateAutoScroll],
  );

  const startProjectDrag = useCallback(
    (e: React.PointerEvent<HTMLDivElement>, projectId: string) => {
      if (displayProjectsRef.current.length < 2) return;
      activeSettleFlushRef.current?.();

      let startY = e.clientY;
      let startScroll = 0;
      let ids: string[] = [];
      let slots: InlineSortSlot[] = [];
      let fromIndex = -1;
      let overIndex = -1;
      let latestDelta = 0;

      const applyDraggedTransform = (delta: number) => {
        const el = projectRowRefs.current.get(projectId);
        if (!el) return;
        draggedProjectElRef.current = el;
        el.style.transform = `translate3d(0, ${delta}px, 0)`;
      };

      const updateAtPointer = (_clientX: number, clientY: number) => {
        const currentScroll = scrollViewportRef.current?.scrollTop ?? startScroll;
        const rawDelta = clientY - startY + (currentScroll - startScroll);
        const clamped = clampInlineSortDelta(slots, fromIndex, rawDelta);
        latestDelta = clamped;
        applyDraggedTransform(clamped);

        const nextOver = computeInlineSortTargetIndex(
          slots,
          fromIndex,
          clamped,
          PROJECT_SORT_MAX_LEAD_IN_PX,
        );
        if (nextOver !== overIndex) {
          overIndex = nextOver;
          setProjectSortState({
            phase: "dragging",
            fromId: projectId,
            fromIndex,
            overIndex: nextOver,
            ids,
            slots,
          });
        }
      };

      const startSettle = (targetOverIdx: number, shouldCommit: boolean) => {
        const targetDelta = computeInlineSortSettleDelta(slots, fromIndex, targetOverIdx);
        const reduced = prefersReducedMotion();

        let finished = false;
        const finalize = () => {
          if (finished) return;
          finished = true;
          clearSettle();
          flushSync(() => {
            if (shouldCommit && targetOverIdx !== fromIndex) {
              const ordered = displayProjectsRef.current.map((d) => d.project);
              const curFrom = ordered.findIndex((p) => p.id === projectId);
              const targetId = ids[targetOverIdx];
              if (curFrom >= 0 && targetId) {
                const [moved] = ordered.splice(curFrom, 1);
                let toIdx = ordered.findIndex((p) => p.id === targetId);
                if (toIdx >= 0) {
                  if (targetOverIdx > fromIndex) toIdx += 1;
                  ordered.splice(toIdx, 0, moved);
                  const keys = ordered.map(
                    (p) =>
                      p.sort_order_key ||
                      p.pinned_project_id ||
                      projectPathScopeKey(p.host_id, p.primary_cwd),
                  );
                  void orbitActions.reorderProjects(keys);
                }
              }
            }
            setProjectSortState(null);
          });
        };

        const draggedEl = projectRowRefs.current.get(projectId) ?? draggedProjectElRef.current;
        if (reduced || !draggedEl || Math.abs(latestDelta - targetDelta) < 0.5) {
          finalize();
          return;
        }

        clearSettle();
        activeSettleFlushRef.current = finalize;
        setProjectSortState({
          phase: "settling",
          fromId: projectId,
          fromIndex,
          overIndex: targetOverIdx,
          ids,
          slots,
        });
        draggedProjectElRef.current = draggedEl;
        settleCancelRef.current = startTransformSettle(
          draggedEl,
          `translate3d(0, ${targetDelta}px, 0)`,
          finalize,
        );
      };

      startPointerDrag(e, {
        thresholdPx: DRAG_THRESHOLD_SORT_PX,
        cursor: "grabbing",
        ignoreSelector: "[data-proj-action], input",
        onActivate: (frame) => {
          const list = displayProjectsRef.current;
          const viewport = scrollViewportRef.current;
          const vpRect = viewport?.getBoundingClientRect() ?? null;
          startScroll = viewport?.scrollTop ?? 0;
          startY = frame.startY;

          const mounted: Array<{ id: string; start: number; size: number }> = [];
          for (const entry of list) {
            const el = projectRowRefs.current.get(entry.project.id);
            if (!el) continue;
            const rect = el.getBoundingClientRect();
            if (rect.height === 0) continue;
            mounted.push({
              id: entry.project.id,
              start: vpRect ? rect.top - vpRect.top + startScroll : rect.top,
              size: rect.height,
            });
          }
          if (mounted.length < 2) return false;

          ids = mounted.map((m) => m.id);
          fromIndex = ids.indexOf(projectId);
          if (fromIndex < 0) return false;
          overIndex = fromIndex;
          slots = buildInlineSortSlots(mounted.map(({ start, size }) => ({ start, size })));
          latestDelta = 0;

          activeScrollTickRef.current = updateAtPointer;
          setProjectSortState({
            phase: "dragging",
            fromId: projectId,
            fromIndex,
            overIndex: fromIndex,
            ids,
            slots,
          });
          return true;
        },
        onMove: (frame) => {
          updateAutoScroll(frame.clientX, frame.clientY);
          updateAtPointer(frame.clientX, frame.clientY);
        },
        onCommit: () => {
          activeScrollTickRef.current = null;
          stopAutoScroll();
          startSettle(overIndex, overIndex !== fromIndex);
        },
        onCancel: () => {
          activeScrollTickRef.current = null;
          stopAutoScroll();
          startSettle(fromIndex, false);
        },
      });
    },
    [clearSettle, scrollViewportRef, startPointerDrag, stopAutoScroll, updateAutoScroll],
  );

  const startPinnedDrag = useCallback(
    (e: React.PointerEvent<HTMLDivElement>, source: PinnedDragSource) => {
      if (pinnedThreadKeysRef.current.length < 2) return;
      activeSettleFlushRef.current?.();

      let startY = e.clientY;
      let startScroll = 0;
      let ids: string[] = [];
      let slots: InlineSortSlot[] = [];
      let fromIndex = -1;
      let overIndex = -1;
      let latestDelta = 0;

      const applyDraggedTransform = (delta: number) => {
        const el = pinnedRowRefs.current.get(source.threadKey);
        if (!el) return;
        draggedPinnedElRef.current = el;
        el.style.transform = `translate3d(0, ${delta}px, 0)`;
      };

      const updateAtPointer = (_clientX: number, clientY: number) => {
        const currentScroll = scrollViewportRef.current?.scrollTop ?? startScroll;
        const rawDelta = clientY - startY + (currentScroll - startScroll);
        const clamped = clampInlineSortDelta(slots, fromIndex, rawDelta);
        latestDelta = clamped;
        applyDraggedTransform(clamped);

        const nextOver = computeInlineSortTargetIndex(slots, fromIndex, clamped);
        if (nextOver !== overIndex) {
          overIndex = nextOver;
          setPinnedSortState({
            phase: "dragging",
            fromId: source.threadKey,
            fromIndex,
            overIndex: nextOver,
            ids,
            slots,
          });
        }
      };

      const startSettle = (targetOverIdx: number, shouldCommit: boolean) => {
        const targetDelta = computeInlineSortSettleDelta(slots, fromIndex, targetOverIdx);
        const reduced = prefersReducedMotion();

        let finished = false;
        const finalize = () => {
          if (finished) return;
          finished = true;
          clearSettle();
          flushSync(() => {
            if (shouldCommit && targetOverIdx !== fromIndex) {
              const fullKeys = pinnedThreadKeysRef.current;
              const targetKey = ids[targetOverIdx];
              const position: "before" | "after" = targetOverIdx > fromIndex ? "after" : "before";
              if (targetKey && !isNoOpLinearDrop(fullKeys, source.threadKey, targetKey, position)) {
                const orderedKeys = fullKeys.filter((k) => k !== source.threadKey);
                let toIdx = orderedKeys.indexOf(targetKey);
                if (toIdx >= 0) {
                  if (position === "after") toIdx += 1;
                  orderedKeys.splice(toIdx, 0, source.threadKey);
                  orbitActions.reorderPinnedThreads(orderedKeys);
                }
              }
            }
            setPinnedSortState(null);
          });
        };

        const draggedEl =
          pinnedRowRefs.current.get(source.threadKey) ?? draggedPinnedElRef.current;
        if (reduced || !draggedEl || Math.abs(latestDelta - targetDelta) < 0.5) {
          finalize();
          return;
        }

        clearSettle();
        activeSettleFlushRef.current = finalize;
        setPinnedSortState({
          phase: "settling",
          fromId: source.threadKey,
          fromIndex,
          overIndex: targetOverIdx,
          ids,
          slots,
        });
        draggedPinnedElRef.current = draggedEl;
        settleCancelRef.current = startTransformSettle(
          draggedEl,
          `translate3d(0, ${targetDelta}px, 0)`,
          finalize,
        );
      };

      startPointerDrag(e, {
        thresholdPx: DRAG_THRESHOLD_SORT_PX,
        cursor: "grabbing",
        ignoreDoubleClick: true,
        ignoreSelector: "[data-thread-action], input",
        onActivate: (frame) => {
          const orderedKeys = pinnedThreadKeysRef.current;
          const viewport = scrollViewportRef.current;
          const vpRect = viewport?.getBoundingClientRect() ?? null;
          startScroll = viewport?.scrollTop ?? 0;
          startY = frame.startY;

          const mounted: Array<{ id: string; start: number; size: number }> = [];
          for (const key of orderedKeys) {
            const el = pinnedRowRefs.current.get(key);
            if (!el) continue;
            const rect = el.getBoundingClientRect();
            if (rect.height === 0) continue;
            mounted.push({
              id: key,
              start: vpRect ? rect.top - vpRect.top + startScroll : rect.top,
              size: rect.height,
            });
          }
          if (mounted.length < 2) return false;

          ids = mounted.map((m) => m.id);
          fromIndex = ids.indexOf(source.threadKey);
          if (fromIndex < 0) return false;
          overIndex = fromIndex;
          slots = buildInlineSortSlots(mounted.map(({ start, size }) => ({ start, size })));
          latestDelta = 0;

          activeScrollTickRef.current = updateAtPointer;
          setPinnedSortState({
            phase: "dragging",
            fromId: source.threadKey,
            fromIndex,
            overIndex: fromIndex,
            ids,
            slots,
          });
          return true;
        },
        onMove: (frame) => {
          updateAutoScroll(frame.clientX, frame.clientY);
          updateAtPointer(frame.clientX, frame.clientY);
        },
        onCommit: () => {
          activeScrollTickRef.current = null;
          stopAutoScroll();
          startSettle(overIndex, overIndex !== fromIndex);
        },
        onCancel: () => {
          activeScrollTickRef.current = null;
          stopAutoScroll();
          startSettle(fromIndex, false);
        },
      });
    },
    [clearSettle, scrollViewportRef, startPointerDrag, stopAutoScroll, updateAutoScroll],
  );

  return {
    liftedThread,
    liftedThreadElRef,
    isAnyDragging: isPointerDragging,
    draggingThread,
    threadInlineSort,
    threadDropIndicator,
    dragOverDropId,
    projectSortState,
    pinnedSortState,
    syncScopeAgentIds,
    registerProjectRow,
    registerThreadRow,
    registerPinnedRow,
    registerWorkspaceHeader,
    registerInboxArea,
    shouldSuppressClick,
    shouldSuppressThreadClick: shouldSuppressClick,
    shouldSuppressProjectClick: shouldSuppressClick,
    shouldSuppressPinnedClick: shouldSuppressClick,
    startThreadDrag,
    startProjectDrag,
    startPinnedDrag,
  };
}
