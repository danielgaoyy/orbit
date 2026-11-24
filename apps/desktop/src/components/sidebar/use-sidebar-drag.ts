import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
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

export interface ThreadDropIndicator {
  scopeKey: string;
  targetAgentId: string;
  position: "before" | "after";
}

export interface ProjectDropIndicator {
  targetId: string;
  position: "before" | "after";
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

type ThreadDropTarget =
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

const AUTO_SCROLL_EDGE_PX = 36;
const AUTO_SCROLL_MAX_SPEED_PX = 10;

function computeEdgeScrollVelocity(clientY: number, viewport: HTMLElement | null): number {
  if (!viewport) return 0;
  const rect = viewport.getBoundingClientRect();
  if (rect.height <= AUTO_SCROLL_EDGE_PX * 2) return 0;
  if (clientY < rect.top + AUTO_SCROLL_EDGE_PX) {
    const ratio = Math.min(1, Math.max(0, (rect.top + AUTO_SCROLL_EDGE_PX - clientY) / AUTO_SCROLL_EDGE_PX));
    return -Math.ceil(ratio * AUTO_SCROLL_MAX_SPEED_PX);
  }
  if (clientY > rect.bottom - AUTO_SCROLL_EDGE_PX) {
    const ratio = Math.min(
      1,
      Math.max(0, (clientY - (rect.bottom - AUTO_SCROLL_EDGE_PX)) / AUTO_SCROLL_EDGE_PX),
    );
    return Math.ceil(ratio * AUTO_SCROLL_MAX_SPEED_PX);
  }
  return 0;
}

/**
 * Pointer-driven drag engine for both Projects and Conversations in `FleetSidebar`.
 *
 * Key engineering invariants:
 * - Zero React re-renders during pointer movement (floating preview uses direct DOM `transform`).
 * - Automatic edge scrolling when dragging near top/bottom of the sidebar viewport.
 * - Cross-group moves preserve `Recent`/`Name` sort mode instead of silently flipping to `manual`.
 * - Works cleanly in macOS Tauri WKWebView where HTML5 Drag-and-Drop is unreliable.
 */
export function useSidebarDrag(params: {
  displayProjects: ReadonlyArray<{ project: SidebarProjectGroupView }>;
  sidebarProjectSort: SidebarProjectSort;
  scrollViewportRef: React.RefObject<HTMLDivElement | null>;
}) {
  const { displayProjects, sidebarProjectSort, scrollViewportRef } = params;

  const displayProjectsRef = useRef(displayProjects);
  displayProjectsRef.current = displayProjects;
  const sortModeRef = useRef(sidebarProjectSort);
  sortModeRef.current = sidebarProjectSort;

  const [draggingThread, setDraggingThread] = useState<ThreadDragSource | null>(null);
  const [threadDropIndicator, setThreadDropIndicator] = useState<ThreadDropIndicator | null>(null);
  const [dragOverDropId, setDragOverDropId] = useState<string | null>(null);
  const [draggingProjectId, setDraggingProjectId] = useState<string | null>(null);
  const [projectDropIndicator, setProjectDropIndicator] = useState<ProjectDropIndicator | null>(null);

  const floatingPreviewRef = useRef<HTMLDivElement | null>(null);
  const lastPreviewPosRef = useRef<{ x: number; y: number }>({ x: 0, y: 0 });
  const projectRowRefs = useRef<Map<string, HTMLDivElement>>(new Map());
  const threadRowRefs = useRef<Map<string, ThreadRowEntry>>(new Map());
  const workspaceHeaderRefs = useRef<Map<string, WorkspaceHeaderEntry>>(new Map());
  const inboxAreaRefs = useRef<Map<string, InboxAreaEntry>>(new Map());
  const scopeAgentIdsRef = useRef<Map<string, string[]>>(new Map());

  const suppressNextProjectClickRef = useRef(false);
  const suppressNextThreadClickRef = useRef(false);
  const projectDragCleanupRef = useRef<(() => void) | null>(null);
  const threadDragCleanupRef = useRef<(() => void) | null>(null);
  const autoScrollRafRef = useRef<number | null>(null);
  const autoScrollVelocityRef = useRef<number>(0);

  const stopAutoScroll = useCallback(() => {
    autoScrollVelocityRef.current = 0;
    if (autoScrollRafRef.current !== null) {
      window.cancelAnimationFrame(autoScrollRafRef.current);
      autoScrollRafRef.current = null;
    }
  }, []);

  const updateAutoScroll = useCallback(
    (clientY: number) => {
      const velocity = computeEdgeScrollVelocity(clientY, scrollViewportRef.current);
      autoScrollVelocityRef.current = velocity;
      if (velocity === 0) {
        stopAutoScroll();
        return;
      }
      if (autoScrollRafRef.current !== null) return;
      const step = () => {
        const v = autoScrollVelocityRef.current;
        const viewport = scrollViewportRef.current;
        if (v === 0 || !viewport) {
          autoScrollRafRef.current = null;
          return;
        }
        viewport.scrollTop += v;
        autoScrollRafRef.current = window.requestAnimationFrame(step);
      };
      autoScrollRafRef.current = window.requestAnimationFrame(step);
    },
    [scrollViewportRef, stopAutoScroll],
  );

  // Sync initial preview coordinates & lift scale immediately before paint, then crisp 110ms fade-in
  useLayoutEffect(() => {
    const el = floatingPreviewRef.current;
    if (!draggingThread || !el) return;
    const { x, y } = lastPreviewPosRef.current;
    el.style.transform = `translate3d(${x + 12}px, ${y - 14}px, 0) scale(1.02)`;
    el.style.opacity = "0";
    const raf = window.requestAnimationFrame(() => {
      if (el.isConnected) {
        el.style.opacity = "1";
      }
    });
    return () => window.cancelAnimationFrame(raf);
  }, [draggingThread]);

  useEffect(() => {
    return () => {
      projectDragCleanupRef.current?.();
      threadDragCleanupRef.current?.();
      stopAutoScroll();
    };
  }, [stopAutoScroll]);

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

  const shouldSuppressThreadClick = useCallback(() => suppressNextThreadClickRef.current, []);
  const shouldSuppressProjectClick = useCallback(() => suppressNextProjectClickRef.current, []);

  const startThreadDrag = useCallback(
    (e: React.PointerEvent<HTMLDivElement>, source: ThreadDragSource) => {
      if (e.button !== 0) return;
      const targetEl = e.target as HTMLElement | null;
      if (targetEl?.closest("[data-thread-action]") || targetEl?.tagName === "INPUT") return;

      const startX = e.clientX;
      const startY = e.clientY;
      let activeDrag = false;
      let currentTarget: ThreadDropTarget = null;
      let lastIndicatorKey = "";
      let lastDropId: string | null = null;

      const onMove = (ev: PointerEvent) => {
        if (!activeDrag) {
          if (Math.hypot(ev.clientX - startX, ev.clientY - startY) < 4) return;
          activeDrag = true;
          suppressNextThreadClickRef.current = true;
          document.body.style.cursor = "grabbing";
          lastPreviewPosRef.current = { x: ev.clientX, y: ev.clientY };
          setDraggingThread(source);
        }

        // Direct DOM transform keeps drag preview locked at 60fps with 0 React re-renders
        lastPreviewPosRef.current = { x: ev.clientX, y: ev.clientY };
        if (floatingPreviewRef.current) {
          floatingPreviewRef.current.style.transform = `translate3d(${ev.clientX + 12}px, ${ev.clientY - 14}px, 0) scale(1.02)`;
        }
        updateAutoScroll(ev.clientY);

        let best: ThreadDropTarget = null;

        // 1. Curated Group headers in the same project (drop into group)
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
            ev.clientX >= rect.left - 8 &&
            ev.clientX <= rect.right + 16 &&
            ev.clientY >= rect.top - 2 &&
            ev.clientY <= rect.bottom + 2
          ) {
            best = { kind: "group", workspaceId: wsEntry.workspaceId };
            break;
          }
        }

        // 2. Thread rows in the same project (reorder or cross-group insert)
        if (!best) {
          const sameScopeEntries: Array<{ entry: ThreadRowEntry; rect: DOMRect }> = [];

          for (const entry of threadRowRefs.current.values()) {
            if (
              entry.hostId !== source.hostId ||
              entry.projectId !== source.projectId ||
              entry.el.closest('.orbit-collapse-grid[data-open="false"]')
            ) {
              continue;
            }
            const rect = entry.el.getBoundingClientRect();
            if (rect.height === 0) continue;
            if (entry.scopeKey === source.scopeKey) {
              sameScopeEntries.push({ entry, rect });
            }
            if (
              ev.clientX >= rect.left - 12 &&
              ev.clientX <= rect.right + 24 &&
              ev.clientY >= rect.top - 4 &&
              ev.clientY <= rect.bottom + 4
            ) {
              if (entry.agentId !== source.agentId) {
                best = {
                  kind: "thread",
                  scopeKey: entry.scopeKey,
                  workspaceId: entry.workspaceId,
                  targetAgentId: entry.agentId,
                  position: ev.clientY < rect.top + rect.height / 2 ? "before" : "after",
                };
              }
              break;
            }
          }

          if (!best && sameScopeEntries.length > 1) {
            sameScopeEntries.sort((a, b) => a.rect.top - b.rect.top);
            const first = sameScopeEntries[0];
            const last = sameScopeEntries[sameScopeEntries.length - 1];
            const inHorizontalBounds =
              ev.clientX >= first.rect.left - 12 && ev.clientX <= first.rect.right + 24;
            if (
              inHorizontalBounds &&
              ev.clientY < first.rect.top &&
              ev.clientY >= first.rect.top - 28 &&
              first.entry.agentId !== source.agentId
            ) {
              best = {
                kind: "thread",
                scopeKey: first.entry.scopeKey,
                workspaceId: first.entry.workspaceId,
                targetAgentId: first.entry.agentId,
                position: "before",
              };
            } else if (
              inHorizontalBounds &&
              ev.clientY > last.rect.bottom &&
              ev.clientY <= last.rect.bottom + 28 &&
              last.entry.agentId !== source.agentId
            ) {
              best = {
                kind: "thread",
                scopeKey: last.entry.scopeKey,
                workspaceId: last.entry.workspaceId,
                targetAgentId: last.entry.agentId,
                position: "after",
              };
            }
          }
        }

        // 3. Unclassified project area (ungroup when dragging out of a curated group)
        if (!best && source.workspaceId !== null) {
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
              ev.clientY >= rect.top - 4 &&
              ev.clientY <= rect.bottom + 12 &&
              ev.clientX <= rect.right + 24
            ) {
              best = {
                kind: "inbox",
                projectId: inboxEntry.projectId,
                scopeKey: inboxEntry.scopeKey,
              };
              break;
            }
          }
        }

        currentTarget = best;

        // Only call setState when the visual drop highlight actually changes
        const nextIndicatorKey =
          best?.kind === "thread"
            ? `${best.scopeKey}::${best.targetAgentId}::${best.position}`
            : "";
        const nextDropId =
          best?.kind === "group"
            ? `ws:${best.workspaceId}`
            : best?.kind === "inbox"
              ? `inbox:${best.projectId}`
              : null;

        if (nextIndicatorKey !== lastIndicatorKey) {
          lastIndicatorKey = nextIndicatorKey;
          setThreadDropIndicator(
            best?.kind === "thread"
              ? {
                  scopeKey: best.scopeKey,
                  targetAgentId: best.targetAgentId,
                  position: best.position,
                }
              : null,
          );
        }
        if (nextDropId !== lastDropId) {
          lastDropId = nextDropId;
          setDragOverDropId(nextDropId);
        }
      };

      const finish = (commit: boolean) => {
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onPointerUp);
        window.removeEventListener("pointercancel", onCancel);
        window.removeEventListener("keydown", onKeyDown);
        threadDragCleanupRef.current = null;
        document.body.style.cursor = "";
        stopAutoScroll();

        if (activeDrag) {
          setDraggingThread(null);
          setThreadDropIndicator(null);
          setDragOverDropId(null);

          if (commit) {
            if (currentTarget?.kind === "thread") {
              const crossGroup = source.workspaceId !== currentTarget.workspaceId;
              if (crossGroup) {
                if (currentTarget.workspaceId) {
                  void orbitActions.moveAgentToWorkspace(
                    source.hostId,
                    source.agentId,
                    currentTarget.workspaceId,
                  );
                } else {
                  void orbitActions.moveAgentToWorkspace(source.hostId, source.agentId, null, {
                    makeUnclassified: true,
                  });
                }
              }

              // Only reorder (and switch to manual sort) when dragging within the same scope
              // or when the user is already explicitly using manual sort mode.
              const shouldPersistOrder = !crossGroup || sortModeRef.current === "manual";
              if (shouldPersistOrder) {
                const fullIds = scopeAgentIdsRef.current.get(currentTarget.scopeKey) ?? [];
                const orderedIds = fullIds.filter((id) => id !== source.agentId);
                let toIdx = orderedIds.indexOf(currentTarget.targetAgentId);
                if (toIdx >= 0) {
                  if (currentTarget.position === "after") toIdx += 1;
                  orderedIds.splice(toIdx, 0, source.agentId);
                } else {
                  orderedIds.unshift(source.agentId);
                }
                orbitActions.reorderThreads(currentTarget.scopeKey, orderedIds);
              }
            } else if (currentTarget?.kind === "group") {
              void orbitActions.moveAgentToWorkspace(
                source.hostId,
                source.agentId,
                currentTarget.workspaceId,
              );
            } else if (currentTarget?.kind === "inbox") {
              void orbitActions.moveAgentToWorkspace(source.hostId, source.agentId, null, {
                makeUnclassified: true,
              });
            }
          }

          window.setTimeout(() => {
            suppressNextThreadClickRef.current = false;
          }, 80);
        }
      };

      const onPointerUp = () => finish(true);
      const onCancel = () => finish(false);
      const onKeyDown = (ev: KeyboardEvent) => {
        if (ev.key === "Escape") finish(false);
      };

      threadDragCleanupRef.current?.();
      threadDragCleanupRef.current = onCancel;
      window.addEventListener("pointermove", onMove);
      window.addEventListener("pointerup", onPointerUp);
      window.addEventListener("pointercancel", onCancel);
      window.addEventListener("keydown", onKeyDown);
    },
    [stopAutoScroll, updateAutoScroll],
  );

  const startProjectDrag = useCallback(
    (e: React.PointerEvent<HTMLDivElement>, projectId: string) => {
      const currentProjects = displayProjectsRef.current;
      if (e.button !== 0 || currentProjects.length < 2) return;
      const targetEl = e.target as HTMLElement | null;
      if (targetEl?.closest("[data-proj-action]") || targetEl?.tagName === "INPUT") return;

      const startY = e.clientY;
      let activeDrag = false;
      let currentDrop: ProjectDropIndicator | null = null;
      let lastDropKey = "";

      const onMove = (ev: PointerEvent) => {
        if (!activeDrag) {
          if (Math.abs(ev.clientY - startY) < 4) return;
          activeDrag = true;
          suppressNextProjectClickRef.current = true;
          setDraggingProjectId(projectId);
          document.body.style.cursor = "grabbing";
        }

        updateAutoScroll(ev.clientY);

        const list = displayProjectsRef.current;
        let best: ProjectDropIndicator | null = null;
        for (const entry of list) {
          const el = projectRowRefs.current.get(entry.project.id);
          if (!el) continue;
          const rect = el.getBoundingClientRect();
          if (ev.clientY >= rect.top - 8 && ev.clientY <= rect.bottom + 8) {
            const mid = rect.top + rect.height / 2;
            best = {
              targetId: entry.project.id,
              position: ev.clientY < mid ? "before" : "after",
            };
            break;
          }
        }
        if (!best && list.length > 0) {
          const firstRect = projectRowRefs.current
            .get(list[0].project.id)
            ?.getBoundingClientRect();
          if (firstRect && ev.clientY < firstRect.top) {
            best = { targetId: list[0].project.id, position: "before" };
          } else {
            best = {
              targetId: list[list.length - 1].project.id,
              position: "after",
            };
          }
        }
        currentDrop = best;
        const nextKey = best ? `${best.targetId}::${best.position}` : "";
        if (nextKey !== lastDropKey) {
          lastDropKey = nextKey;
          setProjectDropIndicator(best);
        }
      };

      const finish = (commit: boolean) => {
        window.removeEventListener("pointermove", onMove);
        window.removeEventListener("pointerup", onPointerUp);
        window.removeEventListener("pointercancel", onCancel);
        window.removeEventListener("keydown", onKeyDown);
        projectDragCleanupRef.current = null;
        document.body.style.cursor = "";
        stopAutoScroll();

        if (activeDrag) {
          setDraggingProjectId(null);
          setProjectDropIndicator(null);
          if (commit && currentDrop && currentDrop.targetId !== projectId) {
            const ordered = displayProjectsRef.current.map((d) => d.project);
            const fromIdx = ordered.findIndex((p) => p.id === projectId);
            if (fromIdx >= 0) {
              const [moved] = ordered.splice(fromIdx, 1);
              let toIdx = ordered.findIndex((p) => p.id === currentDrop?.targetId);
              if (toIdx >= 0) {
                if (currentDrop.position === "after") toIdx += 1;
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
          window.setTimeout(() => {
            suppressNextProjectClickRef.current = false;
          }, 80);
        }
      };

      const onPointerUp = () => finish(true);
      const onCancel = () => finish(false);
      const onKeyDown = (ev: KeyboardEvent) => {
        if (ev.key === "Escape") finish(false);
      };

      projectDragCleanupRef.current?.();
      projectDragCleanupRef.current = onCancel;
      window.addEventListener("pointermove", onMove);
      window.addEventListener("pointerup", onPointerUp);
      window.addEventListener("pointercancel", onCancel);
      window.addEventListener("keydown", onKeyDown);
    },
    [stopAutoScroll, updateAutoScroll],
  );

  return {
    draggingThread,
    threadDropIndicator,
    dragOverDropId,
    draggingProjectId,
    projectDropIndicator,
    floatingPreviewRef,
    syncScopeAgentIds,
    registerProjectRow,
    registerThreadRow,
    registerWorkspaceHeader,
    registerInboxArea,
    shouldSuppressThreadClick,
    shouldSuppressProjectClick,
    startThreadDrag,
    startProjectDrag,
  };
}
