import {
  t,
} from "@/lib/i18n";
import {
  normalizeLineRange,
  normalizeWorkspacePathCandidate,
  resolveUniqueWorkspaceFileSuffix,
} from "@/lib/file-links";
import {
  mergeGitReviewSnapshot,
  normalizeCwdForCompare,
  upsertSubagentSidePanelTab,
} from "@/lib/stage-cache";
import {
  clampSidePanelPrefWidth,
  computeEqualizedSidePanelWidth,
} from "@/lib/workspace-layout";
import {
  MAX_CONCURRENT_FILE_PREFETCHES,
  isHeavyHoverPrefetchPath,
} from "@/lib/file-prefetch";
import {
  type ExternalEditorKind,
  type GitReviewView,
  type WorkspaceFileView,
  formatProviderLabel,
  type ProviderKind,
  type SpawnWorkspacePeerRequest,
  orbitIpc,
} from "@/lib/orbit-ipc";
import {
  MAX_FILE_CACHE_BYTES,
  MAX_FILE_CACHE_ENTRIES,
  basenameFromPath,
  composerScopeKey,
  estimateFileCacheEntryBytes,
  fileCacheKey,
  hasVisibleActiveTerminal,
  normalizeEditorText,
  normalizeLineEndingKind,
  sideQuestionScopeKey,
} from "../store-helpers";
import type { OrbitStoreContext } from "../store-runtime";
import type {
  DiffLayoutMode,
  FileEditorDraftState,
  FileViewCacheEntry,
  OverviewSectionId,
  ReviewScope,
  SidePanelTab,
  SideQuestionEntry,
} from "../store-types";

export function createWorkspaceSlice(ctx: OrbitStoreContext) {
  const {
    bumpFileLoadSeq,
    draftPersistence,
    fileLoadSeqByKey,
    getState,
    notifyToast,
    setState,
  } = ctx;

  const inFlightFileReads = new Map<string, Promise<WorkspaceFileView>>();
  const inFlightPrefetchKeys = new Set<string>();
  const cancelledSideQuestionIds = new Set<string>();
  let nextFileFocusNonce = 1;

  function isUnchangedWorkspaceFileView(
    prev: WorkspaceFileView,
    next: WorkspaceFileView,
  ): boolean {
    if (
      prev.resolved_path !== next.resolved_path ||
      prev.content_kind !== next.content_kind ||
      prev.size_bytes !== next.size_bytes ||
      prev.truncated !== next.truncated ||
      prev.writable !== next.writable ||
      prev.line_ending !== next.line_ending
    ) {
      return false;
    }
    if (prev.etag && next.etag) {
      return prev.etag === next.etag;
    }
    return prev.mtime_unix_ms === next.mtime_unix_ms && prev.content === next.content;
  }

  function startOrJoinFileRead(
    cKey: string,
    hostId: string,
    cwd: string,
    path: string,
    forceReload: boolean,
  ): Promise<WorkspaceFileView> {
    if (!forceReload) {
      const existing = inFlightFileReads.get(cKey);
      if (existing) return existing;
    }
    const promise = orbitIpc.readWorkspaceFile(hostId, cwd, path);
    inFlightFileReads.set(cKey, promise);
    void promise.finally(() => {
      if (inFlightFileReads.get(cKey) === promise) {
        inFlightFileReads.delete(cKey);
      }
    });
    return promise;
  }

  function commitReadyFileView(
    s: ReturnType<typeof getState>,
    cKey: string,
    data: WorkspaceFileView,
  ): ReturnType<typeof getState> {
    const prevEntry = s.fileViewCache[cKey];
    if (
      prevEntry?.status === "ready" &&
      prevEntry.data &&
      isUnchangedWorkspaceFileView(prevEntry.data, data)
    ) {
      if (!prevEntry.stale) {
        return s;
      }
      return {
        ...s,
        fileViewCache: {
          ...s.fileViewCache,
          [cKey]: { status: "ready", data: prevEntry.data },
        },
      };
    }

    // Maintain a bounded LRU file cache capped by BOTH 20 entries and ~64 MiB total memory
    // (so repeated 25 MiB PDF/video previews never blow renderer RAM), while never evicting
    // files with active editor drafts.
    const nextEntry: FileViewCacheEntry = { status: "ready", data };
    const pinnedEntries: Array<[string, FileViewCacheEntry]> = [];
    const evictableEntries: Array<[string, FileViewCacheEntry]> = [];
    for (const [k, v] of Object.entries(s.fileViewCache)) {
      if (k === cKey) continue;
      if (k in s.fileEditorDrafts) {
        pinnedEntries.push([k, v]);
      } else {
        evictableEntries.push([k, v]);
      }
    }
    let totalBytes = estimateFileCacheEntryBytes(nextEntry);
    for (const [, v] of pinnedEntries) {
      totalBytes += estimateFileCacheEntryBytes(v);
    }
    const maxEvictable = Math.max(0, MAX_FILE_CACHE_ENTRIES - 1 - pinnedEntries.length);
    const keptReverse: Array<[string, FileViewCacheEntry]> = [];
    for (let i = evictableEntries.length - 1; i >= 0; i -= 1) {
      if (keptReverse.length >= maxEvictable) break;
      const [k, v] = evictableEntries[i];
      const entryBytes = estimateFileCacheEntryBytes(v);
      if (totalBytes + entryBytes > MAX_FILE_CACHE_BYTES) continue;
      totalBytes += entryBytes;
      keptReverse.push([k, v]);
    }
    keptReverse.reverse();
    const nextCache: Record<string, FileViewCacheEntry> = {};
    for (const [k, v] of pinnedEntries) {
      nextCache[k] = v;
    }
    for (const [k, v] of keptReverse) {
      nextCache[k] = v;
    }
    nextCache[cKey] = nextEntry;

    // If this file is currently open in Edit mode with no unsaved changes, keep its draft synced with disk
    let nextDrafts = s.fileEditorDrafts;
    const existingDraft = s.fileEditorDrafts[cKey];
    if (existingDraft && !existingDraft.dirty && data.content_kind === "text") {
      const normalized = normalizeEditorText(data.content);
      nextDrafts = {
        ...s.fileEditorDrafts,
        [cKey]: {
          ...existingDraft,
          content: normalized,
          baseContent: normalized,
          baseEtag: data.etag ?? null,
          lineEnding: normalizeLineEndingKind(data.line_ending),
          conflict: null,
          saveError: null,
        },
      };
    }
    return {
      ...s,
      fileViewCache: nextCache,
      fileEditorDrafts: nextDrafts,
    };
  }

  async function hydrateSubagentStage(hostId: string, agentId: string): Promise<void> {
    if (!getState().stages[`${hostId}::${agentId}`]?.turns.length) {
      void ctx.actions.pullStageDelta(hostId, agentId, true).catch(() => undefined);
    }
    await orbitIpc.openAgentTimeline(hostId, agentId);
    await ctx.actions.pullStageDelta(hostId, agentId, false);
  }

  return {
    hydrateSubagentStage,


    // =========================================================================
    // Unified Right Side Panel (Multi-Tab Workbench) & Stage Overview Actions
    // =========================================================================

    toggleSidePanel(openOverride?: boolean): void {
      const wasOpen = getState().sidePanelOpen;
      const nextOpen = openOverride ?? !wasOpen;

      setState((s) => {
        let nextSidebarOpen = s.sidebarOpen;
        let nextYielded = s.sidebarAutoYieldedForSidePanel;

        if (nextOpen) {
          if (s.isTightForDualSidebars && s.sidebarOpen) {
            nextSidebarOpen = false;
            nextYielded = true;
          }
        } else if (s.sidebarAutoYieldedForSidePanel && !s.isCompactViewport) {
          nextSidebarOpen = s.sidebarDesktopOpenPref;
          nextYielded = false;
        }

        const willShowLaunchpad =
          nextOpen && (s.sidePanelTabs.length === 0 || !s.activeSidePanelTabId);

        return {
          ...s,
          sidePanelOpen: nextOpen,
          overviewPopoverOpen: nextOpen ? false : s.overviewPopoverOpen,
          sidebarOpen: nextSidebarOpen,
          sidebarAutoYieldedForSidePanel: nextYielded,
          terminalFocused: nextOpen ? s.terminalFocused : false,
          sidePanelLaunchpadFocusNonce: willShowLaunchpad
            ? s.sidePanelLaunchpadFocusNonce + 1
            : s.sidePanelLaunchpadFocusNonce,
        };
      });

      const afterToggle = getState();
      if (nextOpen && !wasOpen) {
        const tab = afterToggle.sidePanelTabs.find((t) => t.id === afterToggle.activeSidePanelTabId);
        const hostId = tab?.hostId ?? afterToggle.selectedHostId;
        if (tab?.kind === "subagent" && tab.childAgentId && hostId) {
          void hydrateSubagentStage(hostId, tab.childAgentId).catch(() => undefined);
        }
      }
      if (nextOpen && afterToggle.selectedHostId && afterToggle.selectedWorkspaceCwd) {
        void ctx.actions.ensureWorkspaceFiles(afterToggle.selectedHostId, afterToggle.selectedWorkspaceCwd);
      }
    },


    openSidePanelNewTab(_opts?: { skipUnsavedGuard?: boolean }): void {
      setState((s) => {
        const shouldYield = !s.sidePanelOpen && s.isTightForDualSidebars && s.sidebarOpen;
        return {
          ...s,
          sidePanelOpen: true,
          overviewPopoverOpen: false,
          activeSidePanelTabId: null,
          terminalFocused: false,
          pendingUnsavedFileAction: null,
          sidePanelLaunchpadFocusNonce: s.sidePanelLaunchpadFocusNonce + 1,
          sidebarOpen: shouldYield ? false : s.sidebarOpen,
          sidebarAutoYieldedForSidePanel: shouldYield ? true : s.sidebarAutoYieldedForSidePanel,
        };
      });

      const afterSnap = getState();
      if (afterSnap.selectedHostId && afterSnap.selectedWorkspaceCwd) {
        void ctx.actions.ensureWorkspaceFiles(afterSnap.selectedHostId, afterSnap.selectedWorkspaceCwd);
      }
    },


    setSidePanelWidth(w: number, persist = true): void {
      setState((s) => ({ ...s, sidePanelWidth: clampSidePanelPrefWidth(w) }), persist);
    },


    equalizeSidePanel(viewportWidth: number, activeLeftWidthPx: number): void {
      const width = computeEqualizedSidePanelWidth(viewportWidth, activeLeftWidthPx);
      setState((s) => ({ ...s, sidePanelWidth: width }), true);
    },


    markOpenFilesStaleForWorkspace(hostId: string, cwd: string): void {
      const prefix = `${hostId}::${cwd}::`;
      const matchingKeys = Object.keys(getState().fileViewCache).filter((k) => k.startsWith(prefix));
      if (matchingKeys.length === 0) return;

      const activeTab = getState().sidePanelTabs.find((t) => t.id === getState().activeSidePanelTabId);
      const activeFileKey =
        getState().sidePanelOpen &&
        activeTab?.kind === "file" &&
        activeTab.hostId &&
        activeTab.cwd &&
        activeTab.filePath
          ? `${activeTab.hostId}::${activeTab.cwd}::${activeTab.filePath}`
          : null;

      setState((s) => {
        const nextCache = { ...s.fileViewCache };
        for (const k of matchingKeys) {
          if (k !== activeFileKey && !s.fileEditorDrafts[k]?.dirty) {
            bumpFileLoadSeq(k);
            inFlightFileReads.delete(k);
            const cur = nextCache[k];
            if (cur?.status === "ready" && cur.data) {
              // Keep cached content warm for instant stale-while-revalidate on next open
              nextCache[k] = { status: "ready", data: cur.data, stale: true };
            } else {
              delete nextCache[k];
            }
          }
        }
        return { ...s, fileViewCache: nextCache };
      });

      if (
        activeFileKey &&
        activeTab?.kind === "file" &&
        activeTab.hostId &&
        activeTab.cwd &&
        activeTab.filePath &&
        activeFileKey.startsWith(prefix) &&
        getState().fileViewCache[activeFileKey]?.status !== "error" &&
        !getState().fileEditorDrafts[activeFileKey]?.dirty
      ) {
        void ctx.actions.loadWorkspaceFile(activeTab.hostId, activeTab.cwd, activeTab.filePath, true);
      }
    },


    focusSidePanelTab(tabId: string, _opts?: { skipUnsavedGuard?: boolean }): void {
      const target = getState().sidePanelTabs.find((t) => t.id === tabId);
      if (!target) return;
      setState((s) => {
        const shouldYield = !s.sidePanelOpen && s.isTightForDualSidebars && s.sidebarOpen;
        const nextTerminalFocused =
          s.terminalFocused &&
          hasVisibleActiveTerminal({
            sidePanelOpen: true,
            sidePanelTabs: s.sidePanelTabs,
            activeSidePanelTabId: tabId,
            terminalTabs: s.terminalTabs,
            activeTerminalTabKey: s.activeTerminalTabKey,
          });
        return {
          ...s,
          sidePanelOpen: true,
          overviewPopoverOpen: false,
          activeSidePanelTabId: tabId,
          terminalFocused: nextTerminalFocused,
          pendingUnsavedFileAction: null,
          sidebarOpen: shouldYield ? false : s.sidebarOpen,
          sidebarAutoYieldedForSidePanel: shouldYield ? true : s.sidebarAutoYieldedForSidePanel,
        };
      });

      const afterFocus = getState();
      if (target.kind === "review" && afterFocus.selectedHostId && afterFocus.selectedWorkspaceCwd) {
        void ctx.actions.refreshGit(
          afterFocus.selectedHostId,
          afterFocus.selectedWorkspaceCwd,
          null,
          afterFocus.diffStaged,
        );
      } else if (target.kind === "files" && afterFocus.selectedHostId && afterFocus.selectedWorkspaceCwd) {
        void ctx.actions.ensureWorkspaceFiles(afterFocus.selectedHostId, afterFocus.selectedWorkspaceCwd);
      } else if (target.kind === "terminal" && afterFocus.selectedHostId) {
        const hostTabs = afterFocus.terminalTabs.filter((t) => t.hostId === afterFocus.selectedHostId);
        if (hostTabs.length === 0) {
          ctx.actions.spawnNewTerminalTab(afterFocus.selectedHostId, afterFocus.selectedWorkspaceCwd || "~");
        } else if (!hostTabs.some((t) => t.key === afterFocus.activeTerminalTabKey)) {
          setState((s) => ({ ...s, activeTerminalTabKey: hostTabs[0].key }));
        }
      } else if (target.kind === "subagent") {
        const subHostId = target.hostId ?? afterFocus.selectedHostId;
        if (subHostId && target.childAgentId) {
          void hydrateSubagentStage(subHostId, target.childAgentId).catch(() => undefined);
        }
      }
    },


    closeSidePanelTab(tabId: string, opts?: { skipUnsavedGuard?: boolean }): void {
      const closingTab = getState().sidePanelTabs.find((t) => t.id === tabId);
      if (!opts?.skipUnsavedGuard && closingTab?.kind === "file" && closingTab.filePath) {
        const closingKey = fileCacheKey(
          closingTab.hostId ?? getState().selectedHostId ?? "",
          closingTab.cwd ?? getState().selectedWorkspaceCwd ?? "~",
          closingTab.filePath,
        );
        if (getState().fileEditorDrafts[closingKey]?.dirty) {
          setState((s) => ({
            ...s,
            sidePanelOpen: true,
            activeSidePanelTabId: tabId,
            pendingUnsavedFileAction: {
              fileKey: closingKey,
              label: `Close ${closingTab.title}`,
              action: { kind: "close_tab", tabId },
            },
          }));
          return;
        }
      }

      let closedPanel = false;
      setState((s) => {
        const idx = s.sidePanelTabs.findIndex((t) => t.id === tabId);
        if (idx < 0) return s;
        const targetTab = s.sidePanelTabs[idx];
        let nextDrafts = s.fileEditorDrafts;
        if (targetTab.kind === "file" && targetTab.filePath) {
          const k = fileCacheKey(
            targetTab.hostId ?? s.selectedHostId ?? "",
            targetTab.cwd ?? s.selectedWorkspaceCwd ?? "~",
            targetTab.filePath,
          );
          if (k in s.fileEditorDrafts) {
            nextDrafts = { ...s.fileEditorDrafts };
            delete nextDrafts[k];
          }
        }
        const remaining = s.sidePanelTabs.filter((t) => t.id !== tabId);
        const nextActive =
          s.activeSidePanelTabId === tabId
            ? (remaining[Math.min(idx, remaining.length - 1)]?.id ?? null)
            : s.activeSidePanelTabId;
        const shouldClosePanel = remaining.length === 0;
        closedPanel = shouldClosePanel;
        const shouldRestoreSidebar =
          shouldClosePanel && s.sidebarAutoYieldedForSidePanel && !s.isCompactViewport;
        const showingLaunchpad = !shouldClosePanel && nextActive === null;
        const nextSidePanelOpen = shouldClosePanel ? false : s.sidePanelOpen;
        const nextTerminalFocused =
          s.terminalFocused &&
          hasVisibleActiveTerminal({
            sidePanelOpen: nextSidePanelOpen,
            sidePanelTabs: remaining,
            activeSidePanelTabId: nextActive,
            terminalTabs: s.terminalTabs,
            activeTerminalTabKey: s.activeTerminalTabKey,
          });
        return {
          ...s,
          sidePanelOpen: nextSidePanelOpen,
          sidebarOpen: shouldRestoreSidebar ? s.sidebarDesktopOpenPref : s.sidebarOpen,
          sidebarAutoYieldedForSidePanel: shouldRestoreSidebar
            ? false
            : s.sidebarAutoYieldedForSidePanel,
          sidePanelTabs: remaining,
          activeSidePanelTabId: nextActive,
          terminalFocused: nextTerminalFocused,
          fileEditorDrafts: nextDrafts,
          pendingUnsavedFileAction: null,
          sidePanelLaunchpadFocusNonce: showingLaunchpad
            ? s.sidePanelLaunchpadFocusNonce + 1
            : s.sidePanelLaunchpadFocusNonce,
        };
      });

      const afterClose = getState();
      if (!closedPanel && afterClose.selectedHostId && afterClose.selectedWorkspaceCwd) {
        void ctx.actions.ensureWorkspaceFiles(afterClose.selectedHostId, afterClose.selectedWorkspaceCwd);
      }
    },


    openReviewTab(opts?: { path?: string | null; scope?: ReviewScope }): void {
      const tabId = "tab:review";
      setState((s) => {
        const exists = s.sidePanelTabs.some((t) => t.id === tabId);
        const nextTabs = exists
          ? s.sidePanelTabs
          : [...s.sidePanelTabs, { id: tabId, kind: "review" as const, title: "Review" }];
        const shouldYield = !s.sidePanelOpen && s.isTightForDualSidebars && s.sidebarOpen;
        return {
          ...s,
          sidePanelOpen: true,
          overviewPopoverOpen: false,
          sidePanelTabs: nextTabs,
          activeSidePanelTabId: tabId,
          terminalFocused: false,
          reviewScope: opts?.scope ?? s.reviewScope,
          selectedDiffPath: opts?.path !== undefined ? opts.path : s.selectedDiffPath,
          sidebarOpen: shouldYield ? false : s.sidebarOpen,
          sidebarAutoYieldedForSidePanel: shouldYield ? true : s.sidebarAutoYieldedForSidePanel,
        };
      });

      const afterReview = getState();
      const effectiveScope = opts?.scope ?? afterReview.reviewScope;
      if (
        effectiveScope === "working_tree" &&
        afterReview.selectedHostId &&
        afterReview.selectedWorkspaceCwd
      ) {
        void ctx.actions.refreshGit(
          afterReview.selectedHostId,
          afterReview.selectedWorkspaceCwd,
          null,
          afterReview.diffStaged,
        );
      }
    },


    toggleReviewTab(openOverride?: boolean): void {
      const tabId = "tab:review";
      const isCurrentlyActive = getState().sidePanelOpen && getState().activeSidePanelTabId === tabId;
      if (openOverride === false || (openOverride === undefined && isCurrentlyActive)) {
        ctx.actions.toggleSidePanel(false);
        return;
      }
      ctx.actions.openReviewTab();
    },


    /** Backward-compatible alias for diff navigation hooks */
    toggleInspector(openOverride?: boolean): void {
      ctx.actions.toggleReviewTab(openOverride);
    },


    /** Backward-compatible alias for jumping straight to a file diff in Review tab */
    openInspectorForFile(path: string, scope: ReviewScope = "session_turns"): void {
      ctx.actions.openReviewTab({ path, scope });
    },


    openFilesExplorerTab(): void {
      const tabId = "tab:files";
      setState((s) => {
        const exists = s.sidePanelTabs.some((t) => t.id === tabId);
        const nextTabs = exists
          ? s.sidePanelTabs
          : [...s.sidePanelTabs, { id: tabId, kind: "files" as const, title: "Files" }];
        const shouldYield = !s.sidePanelOpen && s.isTightForDualSidebars && s.sidebarOpen;
        return {
          ...s,
          sidePanelOpen: true,
          overviewPopoverOpen: false,
          sidePanelTabs: nextTabs,
          activeSidePanelTabId: tabId,
          terminalFocused: false,
          sidebarOpen: shouldYield ? false : s.sidebarOpen,
          sidebarAutoYieldedForSidePanel: shouldYield ? true : s.sidebarAutoYieldedForSidePanel,
        };
      });

      const afterSnap = getState();
      if (afterSnap.selectedHostId && afterSnap.selectedWorkspaceCwd) {
        void ctx.actions.ensureWorkspaceFiles(afterSnap.selectedHostId, afterSnap.selectedWorkspaceCwd);
      }
    },


    openFileInSidePanel(
      rawPath: string,
      opts?: {
        focusLine?: number | null;
        focusEndLine?: number | null;
        hostId?: string;
        cwd?: string;
        fallbackPath?: string;
        initialMode?: "view" | "edit";
        skipUnsavedGuard?: boolean;
      },
    ): void {
      const hostId = opts?.hostId ?? getState().selectedHostId;
      const cwd = opts?.cwd ?? getState().selectedWorkspaceCwd ?? "~";
      const normalizedCandidate = normalizeWorkspacePathCandidate(rawPath, cwd);
      if (!hostId || !normalizedCandidate) return;

      const cachedFiles = getState().workspaceFilesCache[`${hostId}::${cwd}`];
      const resolvedFromCache =
        !normalizedCandidate.startsWith("/") && !normalizedCandidate.startsWith("~/")
          ? resolveUniqueWorkspaceFileSuffix(normalizedCandidate, cachedFiles)
          : null;
      const cleanPath = resolvedFromCache ?? normalizedCandidate;

      const focusRange = normalizeLineRange(opts?.focusLine, opts?.focusEndLine);
      const hasExplicitFocus = opts?.focusLine !== undefined || opts?.focusEndLine !== undefined;
      const focusNonce = focusRange.startLine != null ? nextFileFocusNonce++ : undefined;

      const tabId = `tab:file:${hostId}::${cwd}::${cleanPath}`;
      const title = basenameFromPath(cleanPath) || cleanPath;

      setState((s) => {
        const existingIdx = s.sidePanelTabs.findIndex((t) => t.id === tabId);
        let nextTabs = s.sidePanelTabs;
        if (existingIdx >= 0) {
          if (hasExplicitFocus) {
            nextTabs = [...s.sidePanelTabs];
            nextTabs[existingIdx] = {
              ...nextTabs[existingIdx],
              focusLine: focusRange.startLine,
              focusEndLine: focusRange.endLine,
              ...(focusNonce !== undefined ? { focusNonce } : {}),
            };
          }
        } else {
          nextTabs = [
            ...s.sidePanelTabs,
            {
              id: tabId,
              kind: "file" as const,
              title,
              filePath: cleanPath,
              hostId,
              cwd,
              focusLine: focusRange.startLine,
              focusEndLine: focusRange.endLine,
              focusNonce,
            },
          ];
        }
        const shouldYield = !s.sidePanelOpen && s.isTightForDualSidebars && s.sidebarOpen;
        return {
          ...s,
          sidePanelOpen: true,
          overviewPopoverOpen: false,
          quickOpenFilesOpen: false,
          sidePanelTabs: nextTabs,
          activeSidePanelTabId: tabId,
          terminalFocused: false,
          pendingUnsavedFileAction: null,
          sidebarOpen: shouldYield ? false : s.sidebarOpen,
          sidebarAutoYieldedForSidePanel: shouldYield ? true : s.sidebarAutoYieldedForSidePanel,
        };
      });

      const replaceFailedFileTab = (resolvedAltPath: string) => {
        const nextTabId = `tab:file:${hostId}::${cwd}::${resolvedAltPath}`;
        const nextTitle = basenameFromPath(resolvedAltPath) || resolvedAltPath;
        setState((s) => {
          if (!s.sidePanelTabs.some((t) => t.id === tabId)) return s;
          const alreadyHasNext = s.sidePanelTabs.some((t) => t.id === nextTabId);
          const nextTabs = alreadyHasNext
            ? s.sidePanelTabs
                .filter((t) => t.id !== tabId)
                .map((t) =>
                  t.id === nextTabId
                    ? {
                        ...t,
                        focusLine: focusRange.startLine ?? t.focusLine ?? null,
                        focusEndLine: focusRange.endLine ?? t.focusEndLine ?? null,
                        ...(focusNonce !== undefined ? { focusNonce } : {}),
                      }
                    : t,
                )
            : s.sidePanelTabs.map((t) =>
                t.id === tabId
                  ? {
                      ...t,
                      id: nextTabId,
                      title: nextTitle,
                      filePath: resolvedAltPath,
                    }
                  : t,
              );
          return {
            ...s,
            sidePanelTabs: nextTabs,
            activeSidePanelTabId:
              s.activeSidePanelTabId === tabId ? nextTabId : s.activeSidePanelTabId,
          };
        });
      };

      void (async () => {
        await ctx.actions.loadWorkspaceFile(hostId, cwd, cleanPath, false);
        const primaryKey = `${hostId}::${cwd}::${cleanPath}`;
        if (getState().fileViewCache[primaryKey]?.status !== "error") {
          if (opts?.initialMode === "edit") {
            ctx.actions.enterFileEditMode(hostId, cwd, cleanPath);
          }
          return;
        }

        const trySuffixResolve = async (candidate: string): Promise<string | null> => {
          if (!cwd || cwd === "~" || candidate.startsWith("/") || candidate.startsWith("~/")) {
            return null;
          }
          await ctx.actions.ensureWorkspaceFiles(hostId, cwd);
          const cacheFiles = getState().workspaceFilesCache[`${hostId}::${cwd}`];
          const match = resolveUniqueWorkspaceFileSuffix(candidate, cacheFiles);
          return match && match !== cleanPath ? match : null;
        };

        const fallbackCandidate = opts?.fallbackPath
          ? normalizeWorkspacePathCandidate(opts.fallbackPath, cwd)
          : null;
        if (fallbackCandidate && fallbackCandidate !== cleanPath) {
          const fallbackFromCache =
            !fallbackCandidate.startsWith("/") && !fallbackCandidate.startsWith("~/")
              ? resolveUniqueWorkspaceFileSuffix(
                  fallbackCandidate,
                  getState().workspaceFilesCache[`${hostId}::${cwd}`],
                )
              : null;
          const directAlt = fallbackFromCache ?? fallbackCandidate;
          await ctx.actions.loadWorkspaceFile(hostId, cwd, directAlt, false);
          const altKey = `${hostId}::${cwd}::${directAlt}`;
          if (getState().fileViewCache[altKey]?.status === "ready") {
            replaceFailedFileTab(directAlt);
            return;
          }
          const suffixFromFallback = await trySuffixResolve(fallbackCandidate);
          if (suffixFromFallback) {
            await ctx.actions.loadWorkspaceFile(hostId, cwd, suffixFromFallback, false);
            if (getState().fileViewCache[`${hostId}::${cwd}::${suffixFromFallback}`]?.status === "ready") {
              replaceFailedFileTab(suffixFromFallback);
            }
          }
          return;
        }

        const suffixMatch = await trySuffixResolve(cleanPath);
        if (suffixMatch) {
          await ctx.actions.loadWorkspaceFile(hostId, cwd, suffixMatch, false);
          if (getState().fileViewCache[`${hostId}::${cwd}::${suffixMatch}`]?.status === "ready") {
            replaceFailedFileTab(suffixMatch);
          }
        }
      })();
    },


    prefetchWorkspaceFile(
      rawPath: string,
      opts?: { hostId?: string; cwd?: string; fallbackPath?: string; intent?: "hover" | "press" },
    ): void {
      const hostId = opts?.hostId ?? getState().selectedHostId;
      const cwd = opts?.cwd ?? getState().selectedWorkspaceCwd ?? "~";
      const normalizedCandidate = normalizeWorkspacePathCandidate(rawPath, cwd);
      if (!hostId || !normalizedCandidate) return;

      const intent = opts?.intent ?? "hover";
      const cachedFiles = getState().workspaceFilesCache[`${hostId}::${cwd}`];
      const resolvedFromCache =
        !normalizedCandidate.startsWith("/") && !normalizedCandidate.startsWith("~/")
          ? resolveUniqueWorkspaceFileSuffix(normalizedCandidate, cachedFiles)
          : null;
      const cleanPath = resolvedFromCache ?? normalizedCandidate;
      if (intent === "hover") {
        if (isHeavyHoverPrefetchPath(cleanPath)) return;
        if (inFlightPrefetchKeys.size >= MAX_CONCURRENT_FILE_PREFETCHES) return;
      }

      const cKey = fileCacheKey(hostId, cwd, cleanPath);
      const existing = getState().fileViewCache[cKey];
      if (
        existing?.status === "loading" ||
        (existing?.status === "ready" && !existing.stale) ||
        inFlightFileReads.has(cKey)
      ) {
        return;
      }

      const baseSeq = fileLoadSeqByKey.get(cKey) ?? 0;
      inFlightPrefetchKeys.add(cKey);
      // Warm quietly in background without touching side-panel state or showing loading spinners;
      // clicking mid-prefetch joins this exact in-flight Promise via `inFlightFileReads`.
      void startOrJoinFileRead(cKey, hostId, cwd, cleanPath, false)
        .then((data) => {
          if ((fileLoadSeqByKey.get(cKey) ?? 0) !== baseSeq) return;
          setState((s) => commitReadyFileView(s, cKey, data));
        })
        .catch(() => {
          // Ignore silent prefetch misses; real click handles fallback path resolution
        })
        .finally(() => {
          inFlightPrefetchKeys.delete(cKey);
        });
    },


    async loadWorkspaceFile(
      hostId: string,
      cwd: string,
      path: string,
      forceReload = false,
    ): Promise<void> {
      const cKey = fileCacheKey(hostId, cwd, path);
      const existing = getState().fileViewCache[cKey];
      if (!forceReload && existing?.status === "ready" && !existing.stale) {
        return;
      }
      if (!forceReload && existing?.status === "loading") {
        const pending = inFlightFileReads.get(cKey);
        if (pending) {
          await pending.catch(() => undefined);
        }
        return;
      }

      const isSilentRevalidate =
        !forceReload &&
        existing?.status === "ready" &&
        Boolean(existing.data) &&
        Boolean(existing.stale);

      const loadSeq = bumpFileLoadSeq(cKey);
      if (!isSilentRevalidate) {
        setState((s) => ({
          ...s,
          fileViewCache: {
            ...s.fileViewCache,
            [cKey]: { status: "loading", data: existing?.data },
          },
        }));
      }

      try {
        const data = await startOrJoinFileRead(cKey, hostId, cwd, path, forceReload);
        if (fileLoadSeqByKey.get(cKey) !== loadSeq) {
          return;
        }
        setState((s) => commitReadyFileView(s, cKey, data));
      } catch (err) {
        if (fileLoadSeqByKey.get(cKey) !== loadSeq) {
          return;
        }
        setState((s) => ({
          ...s,
          fileViewCache: {
            ...s.fileViewCache,
            [cKey]: { status: "error", error: String(err) },
          },
        }));
      }
    },


    enterFileEditMode(hostId: string, cwd: string, path: string): void {
      const cKey = fileCacheKey(hostId, cwd, path);
      const data = getState().fileViewCache[cKey]?.data;
      if (!data || data.content_kind !== "text" || data.truncated || data.writable === false) {
        return;
      }
      const normalized = normalizeEditorText(data.content);
      const lineEnding = normalizeLineEndingKind(data.line_ending);
      setState((s) => {
        const existing = s.fileEditorDrafts[cKey];
        if (existing) {
          return {
            ...s,
            fileEditorDrafts: {
              ...s.fileEditorDrafts,
              [cKey]: { ...existing, mode: "edit" },
            },
          };
        }
        return {
          ...s,
          fileEditorDrafts: {
            ...s.fileEditorDrafts,
            [cKey]: {
              hostId,
              cwd,
              path,
              content: normalized,
              baseEtag: data.etag ?? null,
              baseContent: normalized,
              lineEnding,
              dirty: false,
              mode: "edit",
              saving: false,
              saveError: null,
              conflict: null,
            },
          },
        };
      });
    },


    requestFileViewMode(hostId: string, cwd: string, path: string): void {
      const cKey = fileCacheKey(hostId, cwd, path);
      const draft = getState().fileEditorDrafts[cKey];
      if (!draft) return;
      if (draft.dirty) {
        setState((s) => ({
          ...s,
          pendingUnsavedFileAction: {
            fileKey: cKey,
            label: "Switch to View mode",
            action: { kind: "switch_to_view", hostId, cwd, path },
          },
        }));
        return;
      }
      setState((s) => {
        const nextDrafts = { ...s.fileEditorDrafts };
        delete nextDrafts[cKey];
        return {
          ...s,
          fileEditorDrafts: nextDrafts,
          pendingUnsavedFileAction:
            s.pendingUnsavedFileAction?.fileKey === cKey ? null : s.pendingUnsavedFileAction,
        };
      });
    },


    requestFileReload(hostId: string, cwd: string, path: string): void {
      const cKey = fileCacheKey(hostId, cwd, path);
      if (getState().fileEditorDrafts[cKey]?.dirty) {
        setState((s) => ({
          ...s,
          pendingUnsavedFileAction: {
            fileKey: cKey,
            label: "Reload from disk",
            action: { kind: "reload", hostId, cwd, path },
          },
        }));
        return;
      }
      void ctx.actions.loadWorkspaceFile(hostId, cwd, path, true);
    },


    updateFileEditorDraft(hostId: string, cwd: string, path: string, nextContent: string): void {
      const cKey = fileCacheKey(hostId, cwd, path);
      const normalizedNext = normalizeEditorText(nextContent);
      setState((s) => {
        const existing = s.fileEditorDrafts[cKey];
        const diskData = s.fileViewCache[cKey]?.data;
        const baseContent = existing?.baseContent ?? normalizeEditorText(diskData?.content ?? "");
        const baseEtag = existing?.baseEtag ?? diskData?.etag ?? null;
        const lineEnding = existing?.lineEnding ?? normalizeLineEndingKind(diskData?.line_ending);
        return {
          ...s,
          fileEditorDrafts: {
            ...s.fileEditorDrafts,
            [cKey]: {
              hostId,
              cwd,
              path,
              content: normalizedNext,
              baseEtag,
              baseContent,
              lineEnding,
              dirty: normalizedNext !== baseContent,
              mode: "edit",
              saving: existing?.saving ?? false,
              saveError: null,
              conflict: existing?.conflict ?? null,
            },
          },
        };
      });
    },


    clearFileEditorError(hostId: string, cwd: string, path: string): void {
      const cKey = fileCacheKey(hostId, cwd, path);
      setState((s) => {
        const existing = s.fileEditorDrafts[cKey];
        if (!existing || !existing.saveError) return s;
        return {
          ...s,
          fileEditorDrafts: {
            ...s.fileEditorDrafts,
            [cKey]: { ...existing, saveError: null },
          },
        };
      });
    },


    discardFileEditorDraft(hostId: string, cwd: string, path: string): void {
      const cKey = fileCacheKey(hostId, cwd, path);
      setState((s) => {
        const nextDrafts = { ...s.fileEditorDrafts };
        delete nextDrafts[cKey];
        return {
          ...s,
          fileEditorDrafts: nextDrafts,
          pendingUnsavedFileAction:
            s.pendingUnsavedFileAction?.fileKey === cKey ? null : s.pendingUnsavedFileAction,
        };
      });
    },


    async saveWorkspaceFile(
      hostId: string,
      cwd: string,
      path: string,
      force = false,
    ): Promise<"saved" | "conflict" | "error"> {
      const cKey = fileCacheKey(hostId, cwd, path);
      const draft = getState().fileEditorDrafts[cKey];
      if (!draft || draft.saving) return "saved";

      const savedNormalizedContent = draft.content;
      const savedLineEnding = draft.lineEnding;
      const wireContent =
        savedLineEnding === "crlf"
          ? savedNormalizedContent.replace(/\n/g, "\r\n")
          : savedNormalizedContent;

      setState((s) => {
        const cur = s.fileEditorDrafts[cKey];
        if (!cur) return s;
        return {
          ...s,
          fileEditorDrafts: {
            ...s.fileEditorDrafts,
            [cKey]: { ...cur, saving: true, saveError: null },
          },
        };
      });

      try {
        const res = await orbitIpc.writeWorkspaceFile({
          hostId,
          cwd,
          path,
          content: wireContent,
          expectedEtag: draft.baseEtag,
          force,
        });

        if (res.conflict) {
          setState((s) => {
            const cur = s.fileEditorDrafts[cKey];
            if (!cur) return s;
            return {
              ...s,
              fileEditorDrafts: {
                ...s.fileEditorDrafts,
                [cKey]: {
                  ...cur,
                  saving: false,
                  saveError: null,
                  conflict: res.conflict ?? null,
                },
              },
            };
          });
          return "conflict";
        }

        const cachedData = getState().fileViewCache[cKey]?.data;
        const language = cachedData?.language ?? "text";
        const [highlightedLines, markdownBlocks] = await Promise.all([
          orbitIpc
            .highlightWorkspaceCode(
              language,
              savedNormalizedContent,
              Math.max(2500, savedNormalizedContent.split("\n").length + 50),
            )
            .catch(() => cachedData?.highlighted_lines ?? []),
          language === "markdown"
            ? orbitIpc
                .parseMarkdownPreview(savedNormalizedContent, false)
                .catch(() => cachedData?.markdown_blocks ?? [])
            : Promise.resolve(cachedData?.markdown_blocks ?? []),
        ]);

        setState((s) => {
          const curDraft = s.fileEditorDrafts[cKey];
          const stillEdited = curDraft && curDraft.content !== savedNormalizedContent;
          const curCache = s.fileViewCache[cKey]?.data;
          const nextCache = curCache
            ? {
                ...s.fileViewCache,
                [cKey]: {
                  status: "ready" as const,
                  data: {
                    ...curCache,
                    content: wireContent,
                    size_bytes: res.size_bytes,
                    etag: res.etag,
                    mtime_unix_ms: res.mtime_unix_ms ?? curCache.mtime_unix_ms ?? null,
                    line_ending: savedLineEnding,
                    highlighted_lines: highlightedLines,
                    markdown_blocks: markdownBlocks,
                  },
                },
              }
            : s.fileViewCache;

          const nextDrafts = curDraft
            ? {
                ...s.fileEditorDrafts,
                [cKey]: {
                  ...curDraft,
                  baseEtag: res.etag,
                  baseContent: savedNormalizedContent,
                  lineEnding: savedLineEnding,
                  dirty: Boolean(stillEdited),
                  saving: false,
                  saveError: null,
                  conflict: null,
                },
              }
            : s.fileEditorDrafts;

          return {
            ...s,
            fileViewCache: nextCache,
            fileEditorDrafts: nextDrafts,
          };
        });

        if (cwd && cwd !== "~") {
          void ctx.actions.refreshGit(hostId, cwd, null, getState().diffStaged);
          void ctx.actions.loadFileDiagnostics(hostId, cwd, path, true);
        }
        notifyToast(`Saved ${basenameFromPath(path) || path}`);
        return "saved";
      } catch (err) {
        setState((s) => {
          const cur = s.fileEditorDrafts[cKey];
          if (!cur) return s;
          return {
            ...s,
            fileEditorDrafts: {
              ...s.fileEditorDrafts,
              [cKey]: {
                ...cur,
                saving: false,
                saveError: String(err),
              },
            },
          };
        });
        return "error";
      }
    },


    async reloadWorkspaceFileFromConflict(hostId: string, cwd: string, path: string): Promise<void> {
      const cKey = fileCacheKey(hostId, cwd, path);
      try {
        const refreshed = await orbitIpc.readWorkspaceFile(hostId, cwd, path);
        const normalized = normalizeEditorText(refreshed.content);
        const lineEnding = normalizeLineEndingKind(refreshed.line_ending);
        setState((s) => ({
          ...s,
          fileViewCache: {
            ...s.fileViewCache,
            [cKey]: { status: "ready", data: refreshed },
          },
          fileEditorDrafts: {
            ...s.fileEditorDrafts,
            [cKey]: {
              hostId,
              cwd,
              path,
              content: normalized,
              baseEtag: refreshed.etag ?? null,
              baseContent: normalized,
              lineEnding,
              dirty: false,
              mode: "edit",
              saving: false,
              saveError: null,
              conflict: null,
            },
          },
        }));
        notifyToast(`Reloaded ${basenameFromPath(path) || path}`);
      } catch (err) {
        // Preserve existing cache & editor buffer so the user retains access to `Copy mine`
        setState((s) => {
          const cur = s.fileEditorDrafts[cKey];
          if (!cur) return s;
          return {
            ...s,
            fileEditorDrafts: {
              ...s.fileEditorDrafts,
              [cKey]: {
                ...cur,
                saving: false,
                saveError: `Reload failed: ${String(err)}`,
              },
            },
          };
        });
      }
    },


    async resolveUnsavedFileAction(choice: "save" | "discard" | "cancel"): Promise<void> {
      const pending = getState().pendingUnsavedFileAction;
      if (!pending) return;

      if (choice === "cancel") {
        setState((s) => ({ ...s, pendingUnsavedFileAction: null }));
        return;
      }

      const draft = getState().fileEditorDrafts[pending.fileKey];
      if (choice === "save" && draft) {
        const outcome = await ctx.actions.saveWorkspaceFile(
          draft.hostId,
          draft.cwd,
          draft.path,
          false,
        );
        if (outcome !== "saved") {
          // Keep user on current file so conflict banner or save error is visible
          setState((s) => ({ ...s, pendingUnsavedFileAction: null }));
          return;
        }
      }

      if (choice === "discard" && draft) {
        ctx.actions.discardFileEditorDraft(draft.hostId, draft.cwd, draft.path);
      } else {
        setState((s) => ({ ...s, pendingUnsavedFileAction: null }));
      }

      const act = pending.action;
      switch (act.kind) {
        case "close_tab":
          ctx.actions.closeSidePanelTab(act.tabId, { skipUnsavedGuard: true });
          break;
        case "switch_to_view":
          ctx.actions.discardFileEditorDraft(act.hostId, act.cwd, act.path);
          break;
        case "reload":
          ctx.actions.discardFileEditorDraft(act.hostId, act.cwd, act.path);
          void ctx.actions.loadWorkspaceFile(act.hostId, act.cwd, act.path, true);
          break;
      }
    },


    async createWorkspaceEntry(
      hostId: string,
      cwd: string,
      path: string,
      isDir: boolean,
      initialContent?: string,
    ): Promise<string> {
      const res = await orbitIpc.createWorkspaceEntry({
        hostId,
        cwd,
        path,
        isDir,
        initialContent: initialContent ?? (isDir ? null : ""),
      });
      const wsKey = `${hostId}::${cwd}`;
      const normalized = res.path;
      if (isDir) {
        setState((s) => {
          const cur = s.workspaceEmptyDirsCache[wsKey] ?? [];
          if (cur.includes(normalized)) return s;
          return {
            ...s,
            workspaceEmptyDirsCache: {
              ...s.workspaceEmptyDirsCache,
              [wsKey]: [...cur, normalized],
            },
          };
        });
      }
      await ctx.actions.ensureWorkspaceFiles(hostId, cwd, true);
      if (!isDir) {
        ctx.actions.openFileInSidePanel(normalized, { hostId, cwd, initialMode: "edit" });
      }
      if (cwd && cwd !== "~") {
        void ctx.actions.refreshGit(hostId, cwd, null, getState().diffStaged);
      }
      return normalized;
    },


    async renameWorkspaceEntry(
      hostId: string,
      cwd: string,
      oldPath: string,
      newPath: string,
    ): Promise<string> {
      const res = await orbitIpc.renameWorkspaceEntry({ hostId, cwd, oldPath, newPath });
      const oldNorm = res.path;
      const newNorm = res.new_path ?? newPath;
      const wsKey = `${hostId}::${cwd}`;
      const remapPath = (p: string): string | null => {
        if (p === oldNorm) return newNorm;
        if (p.startsWith(`${oldNorm}/`)) return `${newNorm}${p.slice(oldNorm.length)}`;
        return null;
      };

      const pathsNeedingHighlightReload: string[] = [];
      const extOf = (p: string): string => {
        const base = basenameFromPath(p);
        const dot = base.lastIndexOf(".");
        return dot > 0 ? base.slice(dot + 1).toLowerCase() : "";
      };

      setState((s) => {
        const prevEmptyDirs = s.workspaceEmptyDirsCache[wsKey] ?? [];
        const nextEmptyDirs = prevEmptyDirs.map((d) => remapPath(d) ?? d);

        const nextTabs = s.sidePanelTabs.map((t) => {
          if (t.kind !== "file" || !t.filePath) return t;
          const tHost = t.hostId ?? s.selectedHostId;
          const tCwd = t.cwd ?? s.selectedWorkspaceCwd;
          if (tHost !== hostId || tCwd !== cwd) return t;
          const mapped = remapPath(t.filePath);
          if (!mapped) return t;
          const oldKey = `${hostId}::${cwd}::${t.filePath}`;
          if (extOf(t.filePath) !== extOf(mapped) && !s.fileEditorDrafts[oldKey]?.dirty) {
            pathsNeedingHighlightReload.push(mapped);
          }
          return {
            ...t,
            id: `tab:file:${hostId}::${cwd}::${mapped}`,
            title: basenameFromPath(mapped) || mapped,
            filePath: mapped,
          };
        });

        let nextActiveTabId = s.activeSidePanelTabId;
        for (let i = 0; i < s.sidePanelTabs.length; i += 1) {
          if (s.sidePanelTabs[i].id === s.activeSidePanelTabId) {
            nextActiveTabId = nextTabs[i]?.id ?? s.activeSidePanelTabId;
            break;
          }
        }

        const prefix = `${hostId}::${cwd}::`;
        const nextCache: Record<string, FileViewCacheEntry> = {};
        for (const [k, v] of Object.entries(s.fileViewCache)) {
          if (k.startsWith(prefix)) {
            const rel = k.slice(prefix.length);
            const mapped = remapPath(rel);
            if (mapped) {
              nextCache[`${prefix}${mapped}`] = v.data
                ? { ...v, data: { ...v.data, path: mapped } }
                : v;
              continue;
            }
          }
          nextCache[k] = v;
        }

        const nextDrafts: Record<string, FileEditorDraftState> = {};
        for (const [k, v] of Object.entries(s.fileEditorDrafts)) {
          if (k.startsWith(prefix)) {
            const rel = k.slice(prefix.length);
            const mapped = remapPath(rel);
            if (mapped) {
              nextDrafts[`${prefix}${mapped}`] = { ...v, path: mapped };
              continue;
            }
          }
          nextDrafts[k] = v;
        }

        // Clear/remap any pending unsaved guard referencing the renamed file or tab
        let nextPending = s.pendingUnsavedFileAction;
        if (nextPending && nextPending.fileKey.startsWith(prefix)) {
          const mappedKeyRel = remapPath(nextPending.fileKey.slice(prefix.length));
          if (mappedKeyRel) {
            nextPending = null;
          }
        }

        return {
          ...s,
          workspaceEmptyDirsCache: {
            ...s.workspaceEmptyDirsCache,
            [wsKey]: nextEmptyDirs,
          },
          sidePanelTabs: nextTabs,
          activeSidePanelTabId: nextActiveTabId,
          fileViewCache: nextCache,
          fileEditorDrafts: nextDrafts,
          pendingUnsavedFileAction: nextPending,
        };
      });

      for (const reloadPath of pathsNeedingHighlightReload) {
        void ctx.actions.loadWorkspaceFile(hostId, cwd, reloadPath, true);
      }

      await ctx.actions.ensureWorkspaceFiles(hostId, cwd, true);
      if (cwd && cwd !== "~") {
        void ctx.actions.refreshGit(hostId, cwd, null, getState().diffStaged);
      }
      return newNorm;
    },


    async deleteWorkspaceEntry(
      hostId: string,
      cwd: string,
      path: string,
      opts?: { allowDirty?: boolean },
    ): Promise<void> {
      const cleanTarget = path.replace(/\\/g, "/").replace(/^\/+|\/+$/g, "");
      const prefix = `${hostId}::${cwd}::`;
      if (!opts?.allowDirty) {
        const hasDirtyMatch = Object.entries(getState().fileEditorDrafts).some(([k, d]) => {
          if (!d.dirty || !k.startsWith(prefix)) return false;
          const rel = k.slice(prefix.length);
          return rel === cleanTarget || rel.startsWith(`${cleanTarget}/`);
        });
        if (hasDirtyMatch) {
          throw new Error("Cannot delete entry while it has unsaved edits in the editor");
        }
      }

      const res = await orbitIpc.deleteWorkspaceEntry({ hostId, cwd, path });
      const delNorm = res.path;
      const wsKey = `${hostId}::${cwd}`;
      const matchesDeleted = (p: string): boolean => p === delNorm || p.startsWith(`${delNorm}/`);

      setState((s) => {
        const prevEmptyDirs = s.workspaceEmptyDirsCache[wsKey] ?? [];
        const nextEmptyDirs = prevEmptyDirs.filter((d) => !matchesDeleted(d));

        const remainingTabs = s.sidePanelTabs.filter((t) => {
          if (t.kind !== "file" || !t.filePath) return true;
          const tHost = t.hostId ?? s.selectedHostId;
          const tCwd = t.cwd ?? s.selectedWorkspaceCwd;
          if (tHost !== hostId || tCwd !== cwd) return true;
          return !matchesDeleted(t.filePath);
        });

        const activeRemoved = !remainingTabs.some((t) => t.id === s.activeSidePanelTabId);
        const nextActive = activeRemoved
          ? (remainingTabs[remainingTabs.length - 1]?.id ?? null)
          : s.activeSidePanelTabId;

        const nextCache: Record<string, FileViewCacheEntry> = {};
        for (const [k, v] of Object.entries(s.fileViewCache)) {
          if (k.startsWith(prefix) && matchesDeleted(k.slice(prefix.length))) continue;
          nextCache[k] = v;
        }

        const nextDrafts: Record<string, FileEditorDraftState> = {};
        for (const [k, v] of Object.entries(s.fileEditorDrafts)) {
          if (k.startsWith(prefix) && matchesDeleted(k.slice(prefix.length))) continue;
          nextDrafts[k] = v;
        }

        const nextPending =
          s.pendingUnsavedFileAction &&
          s.pendingUnsavedFileAction.fileKey.startsWith(prefix) &&
          matchesDeleted(s.pendingUnsavedFileAction.fileKey.slice(prefix.length))
            ? null
            : s.pendingUnsavedFileAction;

        return {
          ...s,
          workspaceEmptyDirsCache: {
            ...s.workspaceEmptyDirsCache,
            [wsKey]: nextEmptyDirs,
          },
          sidePanelTabs: remainingTabs,
          activeSidePanelTabId: nextActive,
          fileViewCache: nextCache,
          fileEditorDrafts: nextDrafts,
          pendingUnsavedFileAction: nextPending,
        };
      });

      await ctx.actions.ensureWorkspaceFiles(hostId, cwd, true);
      if (cwd && cwd !== "~") {
        void ctx.actions.refreshGit(hostId, cwd, null, getState().diffStaged);
      }
    },


    openBrowserTab(initialUrl?: string): void {
      const tabId = "tab:browser";
      const rawUrl = initialUrl?.trim() ?? "";
      let cleanUrl = "";
      if (rawUrl) {
        try {
          const parsed = new URL(rawUrl);
          if (parsed.protocol === "http:" || parsed.protocol === "https:") {
            cleanUrl = parsed.toString();
          }
        } catch {
          cleanUrl = "";
        }
      }
      setState((s) => {
        const existingIdx = s.sidePanelTabs.findIndex((t) => t.id === tabId);
        let nextTabs = s.sidePanelTabs;
        if (existingIdx >= 0) {
          if (cleanUrl) {
            nextTabs = [...s.sidePanelTabs];
            nextTabs[existingIdx] = {
              ...nextTabs[existingIdx],
              url: cleanUrl,
            };
          }
        } else {
          nextTabs = [
            ...s.sidePanelTabs,
            {
              id: tabId,
              kind: "browser" as const,
              title: "Browser",
              url: cleanUrl,
            },
          ];
        }
        const shouldYield = !s.sidePanelOpen && s.isTightForDualSidebars && s.sidebarOpen;
        return {
          ...s,
          sidePanelOpen: true,
          overviewPopoverOpen: false,
          sidePanelTabs: nextTabs,
          activeSidePanelTabId: tabId,
          terminalFocused: false,
          sidebarOpen: shouldYield ? false : s.sidebarOpen,
          sidebarAutoYieldedForSidePanel: shouldYield ? true : s.sidebarAutoYieldedForSidePanel,
        };
      });
    },


    updateBrowserTabUrl(tabId: string, url: string): void {
      let safeUrl = "";
      if (url.trim()) {
        try {
          const parsed = new URL(url.trim());
          if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return;
          safeUrl = parsed.toString();
        } catch {
          return;
        }
      }
      setState((s) => {
        const idx = s.sidePanelTabs.findIndex((t) => t.id === tabId);
        if (idx < 0) return s;
        const nextTabs = [...s.sidePanelTabs];
        nextTabs[idx] = { ...nextTabs[idx], url: safeUrl };
        return { ...s, sidePanelTabs: nextTabs };
      });
    },


    openSideQuestionTab(opts?: { entryId?: string }): void {
      const tabId = "tab:side_question";
      setState((s) => {
        const exists = s.sidePanelTabs.some((t) => t.id === tabId);
        const nextTabs = exists
          ? s.sidePanelTabs.map((t) =>
              t.id === tabId
                ? {
                    ...t,
                    sideQuestionEntryId:
                      opts?.entryId !== undefined ? opts.entryId : t.sideQuestionEntryId,
                  }
                : t,
            )
          : [
              ...s.sidePanelTabs,
              {
                id: tabId,
                kind: "side_question" as const,
                title: "Side Question",
                sideQuestionEntryId: opts?.entryId,
              },
            ];
        const shouldYield = !s.sidePanelOpen && s.isTightForDualSidebars && s.sidebarOpen;
        return {
          ...s,
          sidePanelOpen: true,
          overviewPopoverOpen: false,
          sidePanelTabs: nextTabs,
          activeSidePanelTabId: tabId,
          terminalFocused: false,
          sideQuestionFocusNonce: s.sideQuestionFocusNonce + 1,
          sidebarOpen: shouldYield ? false : s.sidebarOpen,
          sidebarAutoYieldedForSidePanel: shouldYield ? true : s.sidebarAutoYieldedForSidePanel,
        };
      });
    },


    toggleSideQuestionTab(openOverride?: boolean): void {
      const tabId = "tab:side_question";
      const isCurrentlyActive = getState().sidePanelOpen && getState().activeSidePanelTabId === tabId;
      if (openOverride === false || (openOverride === undefined && isCurrentlyActive)) {
        ctx.actions.toggleSidePanel(false);
        return;
      }
      ctx.actions.openSideQuestionTab();
    },


    setSideQuestionDraft(text: string): void {
      const scope = sideQuestionScopeKey(getState());
      setState((s) => ({
        ...s,
        sideQuestionDrafts: { ...s.sideQuestionDrafts, [scope]: text },
      }));
    },


    clearSideQuestions(): void {
      const scope = sideQuestionScopeKey(getState());
      for (const e of getState().sideQuestionsByScope[scope] ?? []) {
        if (e.status === "streaming") cancelledSideQuestionIds.add(e.id);
      }
      setState((s) => {
        const next = { ...s.sideQuestionsByScope };
        delete next[scope];
        return { ...s, sideQuestionsByScope: next };
      });
    },


    cancelSideQuestion(scopeOverride?: string): void {
      const scope = scopeOverride ?? sideQuestionScopeKey(getState());
      const list = getState().sideQuestionsByScope[scope];
      if (!list) return;
      const streamingIds = list.filter((e) => e.status === "streaming").map((e) => e.id);
      if (streamingIds.length === 0) return;
      for (const id of streamingIds) {
        cancelledSideQuestionIds.add(id);
      }
      setState((s) => {
        const cur = s.sideQuestionsByScope[scope];
        if (!cur) return s;
        return {
          ...s,
          sideQuestionsByScope: {
            ...s.sideQuestionsByScope,
            [scope]: cur.map((e) =>
              streamingIds.includes(e.id)
                ? {
                    ...e,
                    status: e.answer.trim() ? ("done" as const) : ("error" as const),
                    error: e.answer.trim() ? null : "Cancelled",
                  }
                : e,
            ),
          },
        };
      });
    },


    promoteSideQuestionToComposer(text: string): void {
      const trimmed = text.trim();
      if (!trimmed) return;
      const key = composerScopeKey(getState());
      const existing = (getState().composerDrafts[key] ?? "").trim();
      const nextDraft = existing ? `${existing}\n\n${trimmed}` : trimmed;
      setState((s) => ({
        ...s,
        composerDrafts: { ...s.composerDrafts, [key]: nextDraft },
      }));
      ctx.actions.focusComposer();
      notifyToast("Copied to main Composer");
    },


    async submitSideQuestion(
      rawQuestion?: string,
      targetOverride?: {
        hostId: string;
        scopeKey?: string;
        isDraftNewSession: boolean;
        agentId: string | null;
        workspaceCwd: string;
        draftProvider?: ProviderKind;
        draftAcpAgentId?: string | null;
        draftModels?: Partial<Record<ProviderKind, string | null>>;
      },
    ): Promise<void> {
      const hostId = targetOverride?.hostId ?? getState().selectedHostId;
      if (!hostId) {
        notifyToast("Connect an Orbit host first");
        return;
      }

      const isDraft = targetOverride ? targetOverride.isDraftNewSession : getState().isDraftNewSession;
      const targetAgentId = targetOverride ? targetOverride.agentId : getState().selectedAgentId;
      const scope =
        targetOverride?.scopeKey ??
        (isDraft || !targetAgentId ? `draft::${hostId}` : `${hostId}::${targetAgentId}`);
      const question = (rawQuestion ?? getState().sideQuestionDrafts[scope] ?? "").trim();
      if (!question) return;

      const detail = getState().hostDetails[hostId];
      const currentAgent =
        !isDraft && targetAgentId
          ? detail?.agents.find((a) => a.agent_id === targetAgentId) ??
            getState().stages[`${hostId}::${targetAgentId}`]?.agent ??
            null
          : null;

      const provider = currentAgent?.provider ?? targetOverride?.draftProvider ?? getState().draftProvider;
      const draftModels = targetOverride?.draftModels ?? getState().draftModels;
      const draftAcpAgentId =
        targetOverride?.draftAcpAgentId !== undefined
          ? targetOverride.draftAcpAgentId
          : getState().draftAcpAgentId;
      const model = currentAgent?.model ?? draftModels[provider] ?? null;
      const cwd = currentAgent?.cwd || targetOverride?.workspaceCwd || getState().selectedWorkspaceCwd || "~";
      const providerLabel =
        currentAgent?.provider_label ??
        (provider === "acp"
          ? (draftAcpAgentId ?? formatProviderLabel("acp"))
          : formatProviderLabel(provider));

      const gitReview = getState().gitReviews[`${hostId}::${cwd}`] ?? null;
      const branchName =
        gitReview?.status?.branch ??
        detail?.workspaces.find((w) => w.cwd === cwd)?.git_branch ??
        null;
      const rustContext =
        !isDraft && targetAgentId
          ? await orbitIpc
              .buildSideQuestionContext(hostId, targetAgentId, 3000)
              .catch(() => "")
          : "";
      const priorSideTurns = (getState().sideQuestionsByScope[scope] ?? [])
        .filter((e) => e.status === "done" && e.answer.trim().length > 0)
        .slice(-4);
      const priorHistoryBlock =
        priorSideTurns.length > 0
          ? `\n\nPrior side-thread Q&A:\n${priorSideTurns
              .map(
                (e, i) =>
                  `Q${i + 1}: ${e.question.slice(0, 400)}\nA${i + 1}: ${e.answer.slice(0, 900)}`,
              )
              .join("\n\n")}`
          : "";
      const baseContext =
        rustContext || `Workspace: ${cwd || "~"}${branchName ? ` (branch: ${branchName})` : ""}`;
      const contextSummary = `${baseContext}${priorHistoryBlock}`;

      const entryId = `sq_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`;
      const newEntry: SideQuestionEntry = {
        id: entryId,
        question,
        answer: "",
        blocks: [],
        status: "streaming",
        error: null,
        createdAtMs: Date.now(),
        providerLabel,
      };

      const stillViewingTarget =
        getState().selectedHostId === hostId &&
        (getState().isDraftNewSession || !getState().selectedAgentId) === (isDraft || !targetAgentId) &&
        (isDraft || !targetAgentId || getState().selectedAgentId === targetAgentId);
      if (!targetOverride || stillViewingTarget) {
        ctx.actions.openSideQuestionTab();
      }
      setState((s) => ({
        ...s,
        sideQuestionDrafts: { ...s.sideQuestionDrafts, [scope]: "" },
        sideQuestionsByScope: {
          ...s.sideQuestionsByScope,
          [scope]: [...(s.sideQuestionsByScope[scope] ?? []), newEntry],
        },
      }));
      draftPersistence.flushNow();

      let accumText = "";
      let parseTimer: ReturnType<typeof setTimeout> | null = null;
      let parseRevision = 0;

      const patchEntry = (mutator: (entry: SideQuestionEntry) => SideQuestionEntry) => {
        setState((s) => {
          const list = s.sideQuestionsByScope[scope];
          if (!list) return s;
          const idx = list.findIndex((item) => item.id === entryId);
          if (idx < 0) return s;
          const nextList = [...list];
          nextList[idx] = mutator(nextList[idx]);
          return {
            ...s,
            sideQuestionsByScope: {
              ...s.sideQuestionsByScope,
              [scope]: nextList,
            },
          };
        });
      };

      const scheduleMarkdownParse = (text: string, streaming: boolean) => {
        const rev = ++parseRevision;
        if (parseTimer) {
          clearTimeout(parseTimer);
          parseTimer = null;
        }
        const run = () => {
          void orbitIpc
            .parseMarkdownPreview(text, streaming)
            .then((blocks) => {
              if (rev !== parseRevision && streaming) return;
              if (blocks.length > 0) {
                patchEntry((e) => ({ ...e, blocks }));
              }
            })
            .catch(() => {});
        };
        if (streaming) {
          parseTimer = setTimeout(run, 85);
        } else {
          run();
        }
      };

      try {
        await orbitIpc.askSideQuestion(
          {
            hostId,
            agentId: currentAgent?.agent_id ?? null,
            cwd,
            provider,
            model,
            question,
            contextSummary,
          },
          (ev) => {
            if (cancelledSideQuestionIds.has(entryId)) {
              if (parseTimer) {
                clearTimeout(parseTimer);
                parseTimer = null;
              }
              return;
            }
            if (ev.kind === "delta") {
              accumText += ev.text_delta;
              patchEntry((e) => ({ ...e, answer: accumText, status: "streaming" }));
              scheduleMarkdownParse(accumText, true);
            } else if (ev.kind === "done") {
              const finalText = ev.full_text || accumText;
              const hasError = Boolean(ev.error && ev.error.trim().length > 0);
              patchEntry((e) => ({
                ...e,
                answer: finalText,
                status: hasError ? "error" : "done",
                error: ev.error,
              }));
              if (finalText.trim()) {
                scheduleMarkdownParse(finalText, false);
              }
            }
          },
        );
      } catch (err) {
        if (parseTimer) {
          clearTimeout(parseTimer);
          parseTimer = null;
        }
        if (!cancelledSideQuestionIds.has(entryId)) {
          patchEntry((e) => ({
            ...e,
            status: "error",
            error: String(err),
          }));
        }
      } finally {
        cancelledSideQuestionIds.delete(entryId);
      }
    },


    openSubagentTab(params: {
      callId: string;
      role: string;
      promptPreview: string;
      childAgentId: string | null;
      childHostId?: string | null;
      providerSubagentId?: string | null;
      parentAgentId?: string | null;
    }): void {
      const snap = getState();
      const parentHostId = snap.selectedHostId;
      const targetHostId = params.childHostId ?? parentHostId;
      const parentAgentId = params.parentAgentId ?? snap.selectedAgentId;
      const hasManagedChild = Boolean(
        targetHostId &&
          params.childAgentId &&
          (snap.stages[`${targetHostId}::${params.childAgentId}`]?.turns.length ||
            snap.hostDetails[targetHostId]?.agents.some((a) => a.agent_id === params.childAgentId)),
      );
      const effectiveChildAgentId = hasManagedChild ? params.childAgentId : null;
      const effectiveProviderSubagentId =
        params.providerSubagentId ?? (!hasManagedChild ? params.childAgentId : null);
      const tabHostId = hasManagedChild ? targetHostId : parentHostId;
      setState((s) => {
        const { tabs: nextTabs, activeTabId } = upsertSubagentSidePanelTab(s.sidePanelTabs, {
          callId: params.callId,
          role: params.role,
          promptPreview: params.promptPreview,
          tabHostId,
          parentHostId,
          parentAgentId,
          effectiveChildAgentId,
          effectiveProviderSubagentId,
        });
        const shouldYield = !s.sidePanelOpen && s.isTightForDualSidebars && s.sidebarOpen;
        return {
          ...s,
          sidePanelOpen: true,
          overviewPopoverOpen: false,
          sidePanelTabs: nextTabs,
          activeSidePanelTabId: activeTabId,
          terminalFocused: false,
          sidebarOpen: shouldYield ? false : s.sidebarOpen,
          sidebarAutoYieldedForSidePanel: shouldYield ? true : s.sidebarAutoYieldedForSidePanel,
        };
      });

      if (tabHostId && effectiveChildAgentId) {
        void hydrateSubagentStage(tabHostId, effectiveChildAgentId).catch(() => undefined);
      }
    },


    openProviderSubagentTimeline(
      hostId: string,
      parentAgentId: string,
      subagentId: string,
    ): Promise<void> {
      return orbitIpc.openProviderSubagentTimeline(hostId, parentAgentId, subagentId);
    },


    setStageCanDockOverview(canDock: boolean): void {
      if (getState().stageCanDockOverview === canDock) return;
      setState((s) => ({
        ...s,
        stageCanDockOverview: canDock,
        overviewPopoverOpen: canDock ? false : s.overviewPopoverOpen,
      }));
    },


    toggleOverviewPanel(openOverride?: boolean): void {
      if (!getState().stageCanDockOverview) {
        const nextPop = openOverride ?? !getState().overviewPopoverOpen;
        setState((s) => ({ ...s, overviewPopoverOpen: nextPop }));
        return;
      }
      setState(
        (s) => ({
          ...s,
          overviewDesktopOpenPref: openOverride ?? !s.overviewDesktopOpenPref,
        }),
        true,
      );
    },


    setOverviewPopoverOpen(open: boolean): void {
      setState((s) => ({ ...s, overviewPopoverOpen: open }));
    },


    toggleOverviewSection(section: OverviewSectionId): void {
      setState(
        (s) => ({
          ...s,
          overviewCollapsedSections: {
            ...s.overviewCollapsedSections,
            [section]: !s.overviewCollapsedSections[section],
          },
        }),
        true,
      );
    },


    recordSessionPullRequest(pr: {
      url: string;
      number?: number | null;
      title?: string | null;
    }): void {
      const snap = getState();
      const hostId = snap.selectedHostId;
      const agentId = snap.selectedAgentId;
      if (!hostId || !agentId || snap.isDraftNewSession || !pr.url?.trim()) return;
      const cleanUrl = pr.url.trim();
      let repo = "";
      let parsedNumber = pr.number ?? 0;
      try {
        const parsed = new URL(cleanUrl);
        const segs = parsed.pathname.split("/").filter(Boolean);
        const dashIdx = segs.indexOf("-");
        if (dashIdx >= 2 && segs[dashIdx + 1] === "merge_requests") {
          repo = `${segs[dashIdx - 2]}/${segs[dashIdx - 1]}`;
          if (!parsedNumber) parsedNumber = Number.parseInt(segs[dashIdx + 2] ?? "0", 10) || 0;
        } else if (segs.length >= 4) {
          repo = `${segs[0]}/${segs[1]}`;
          if (!parsedNumber) parsedNumber = Number.parseInt(segs[3] ?? "0", 10) || 0;
        }
      } catch {
        // Keep fallback fields when URL parsing fails
      }
      const key = `${hostId}::${agentId}`;
      const entry = {
        url: cleanUrl,
        number: parsedNumber,
        repo,
        title: pr.title?.trim() || null,
        last_seq: Number.MAX_SAFE_INTEGER - 1,
      };
      setState((s) => {
        const existing = s.sessionCreatedPrs[key] ?? [];
        const deduped = existing.filter(
          (item) => item.url.toLowerCase() !== cleanUrl.toLowerCase(),
        );
        return {
          ...s,
          sessionCreatedPrs: {
            ...s.sessionCreatedPrs,
            [key]: [entry, ...deduped].slice(0, 12),
          },
        };
      });
    },


    setQuickOpenFilesOpen(open: boolean): void {
      setState((s) => ({ ...s, quickOpenFilesOpen: open }));
      const snap = getState();
      if (open && snap.selectedHostId && snap.selectedWorkspaceCwd) {
        void ctx.actions.ensureWorkspaceFiles(snap.selectedHostId, snap.selectedWorkspaceCwd);
      }
    },


    setReviewScope(scope: ReviewScope): void {
      setState((s) => ({ ...s, reviewScope: scope, selectedDiffPath: null }));
      const snap = getState();
      if (scope === "working_tree" && snap.selectedHostId && snap.selectedWorkspaceCwd) {
        void ctx.actions.refreshGit(
          snap.selectedHostId,
          snap.selectedWorkspaceCwd,
          null,
          snap.diffStaged,
        );
      }
    },


    setDiffStaged(staged: boolean): void {
      setState((s) => ({ ...s, diffStaged: staged, selectedDiffPath: null }));
      const snap = getState();
      if (snap.selectedHostId && snap.selectedWorkspaceCwd) {
        void ctx.actions.refreshGit(
          snap.selectedHostId,
          snap.selectedWorkspaceCwd,
          null,
          staged,
        );
      }
    },


    setDiffLayout(mode: DiffLayoutMode): void {
      setState((s) => ({ ...s, diffLayout: mode }), true);
    },


    selectDiffFile(path: string | null): void {
      setState((s) => ({ ...s, selectedDiffPath: path }));
      const snap = getState();
      if (
        snap.reviewScope === "working_tree" &&
        snap.selectedHostId &&
        snap.selectedWorkspaceCwd
      ) {
        const rKey = `${snap.selectedHostId}::${snap.selectedWorkspaceCwd}`;
        const cached = snap.gitReviews[rKey];
        // Fast-path: if we already have the full diff cached and the requested file is present
        // (or user clicked "All"), filter client-side in 0ms without spawning git subprocesses.
        if (path === null && cached && cached.selected_path === null && cached.diff_files.length > 0) {
          return;
        }
        if (
          path !== null &&
          cached &&
          cached.selected_path === null &&
          cached.diff_files.some((f) => f.path === path)
        ) {
          return;
        }
        void ctx.actions.refreshGit(
          snap.selectedHostId,
          snap.selectedWorkspaceCwd,
          path,
          snap.diffStaged,
        );
      }
    },


    async refreshGit(
      hostId: string,
      cwd: string,
      path: string | null,
      staged: boolean,
    ): Promise<void> {
      await orbitIpc.fetchGitStatus(hostId, cwd);
      await orbitIpc.fetchGitDiff(hostId, cwd, path, staged);
      await ctx.actions.loadGitReviewSnapshot(hostId, cwd, path);
    },


    async refreshGitReview(
      hostId: string,
      cwd: string,
      opts?: { path?: string | null; staged?: boolean; clearSelectedPath?: boolean },
    ): Promise<void> {
      const staged = opts?.staged ?? getState().diffStaged;
      const path = opts?.clearSelectedPath ? null : (opts?.path ?? getState().selectedDiffPath);
      if (opts?.clearSelectedPath && getState().selectedDiffPath !== null) {
        setState((s) => ({ ...s, selectedDiffPath: null }));
      }
      await ctx.actions.refreshGit(hostId, cwd, path, staged);
    },


    async loadGitReviewSnapshot(hostId: string, cwd: string, path: string | null): Promise<void> {
      const gr = await orbitIpc.getGitReview(hostId, cwd, path);
      const rKey = `${hostId}::${cwd}`;
      const normIncomingCwd = normalizeCwdForCompare(cwd);
      setState((s) => {
        const mergedPrimary = mergeGitReviewSnapshot(s.gitReviews[rKey], gr);
        const nextGitReviews: Record<string, GitReviewView> = {
          ...s.gitReviews,
          [rKey]: mergedPrimary,
        };
        // Keep selectedWorkspaceCwd cache synchronized even if trailing slashes differ
        if (
          s.selectedHostId === hostId &&
          s.selectedWorkspaceCwd &&
          s.selectedWorkspaceCwd !== cwd &&
          normalizeCwdForCompare(s.selectedWorkspaceCwd) === normIncomingCwd
        ) {
          const selectedKey = `${hostId}::${s.selectedWorkspaceCwd}`;
          nextGitReviews[selectedKey] = mergeGitReviewSnapshot(s.gitReviews[selectedKey], gr);
        }
        return {
          ...s,
          gitReviews: nextGitReviews,
        };
      });
    },


    async openWorkspaceInEditor(preferredKind?: ExternalEditorKind): Promise<void> {
      const hostId = getState().selectedHostId;
      if (!hostId) return;
      const detail = getState().hostDetails[hostId];
      const cwd = getState().selectedWorkspaceCwd || "~";

      const chosenKind =
        preferredKind ??
        getState().preferredEditor ??
        getState().externalEditors.find((e) => e.installed)?.kind ??
        "cursor";

      // Infer whether this host is local loopback or remote SSH
      const isLocal = detail?.host.direct_endpoints.some(
        (ep) => ep.includes("127.0.0.1") || ep.includes("localhost"),
      );
      let sshTarget: string | null = null;
      if (!isLocal && detail) {
        const matchedSsh = getState().sshHosts.find(
          (s) =>
            s.alias.toLowerCase() === detail.host.display_name.toLowerCase() ||
            s.host_name.toLowerCase() === detail.host.display_name.toLowerCase(),
        );
        sshTarget = matchedSsh?.alias ?? null;
      }

      try {
        await orbitIpc.openInEditor(chosenKind, cwd, sshTarget);
      } catch (err) {
        notifyToast(String(err));
      }
    },


    openArtifactsTab(opts?: {
      hostId?: string;
      workspaceId?: string;
      artifactId?: string;
    }): void {
      const resolvedHostId = opts?.hostId || getState().selectedHostId || "";
      const detail = resolvedHostId ? getState().hostDetails[resolvedHostId] : null;
      const activeAgent =
        !getState().isDraftNewSession && getState().selectedAgentId
          ? detail?.agents.find((a) => a.agent_id === getState().selectedAgentId) ?? null
          : null;
      const resolvedWorkspaceId = opts?.workspaceId ?? activeAgent?.workspace_id ?? "";
      const tabId = "tab:artifacts";
      const nonce = Date.now();

      setState((s) => {
        const exists = s.sidePanelTabs.some((t) => t.id === tabId);
        const nextTab: SidePanelTab = {
          id: tabId,
          kind: "artifacts",
          title: "Artifacts",
          hostId: resolvedHostId || undefined,
          workspaceId: resolvedWorkspaceId || undefined,
          artifactId: opts?.artifactId,
          focusNonce: nonce,
        };
        const nextTabs = exists
          ? s.sidePanelTabs.map((t) =>
              t.id === tabId
                ? {
                    ...t,
                    hostId: resolvedHostId || t.hostId,
                    workspaceId: resolvedWorkspaceId || t.workspaceId,
                    artifactId: opts?.artifactId ?? t.artifactId,
                    focusNonce: nonce,
                  }
                : t,
            )
          : [...s.sidePanelTabs, nextTab];
        const shouldYield = !s.sidePanelOpen && s.isTightForDualSidebars && s.sidebarOpen;
        return {
          ...s,
          sidePanelOpen: true,
          overviewPopoverOpen: false,
          sidePanelTabs: nextTabs,
          activeSidePanelTabId: tabId,
          terminalFocused: false,
          sidebarOpen: shouldYield ? false : s.sidebarOpen,
          sidebarAutoYieldedForSidePanel: shouldYield ? true : s.sidebarAutoYieldedForSidePanel,
        };
      });
      if (resolvedHostId) {
        void orbitIpc.listArtifacts(resolvedHostId, null, null).catch(() => {});
      }
    },


    openWorkspaceBoardTab(hostId: string, workspaceId: string, title?: string): void {
      const tabId = `tab:board:${hostId}::${workspaceId}`;
      const resolvedTitle = title ? `Board · ${title}` : "Workspace Board";
      setState((s) => {
        const exists = s.sidePanelTabs.some((t) => t.id === tabId);
        const nextTabs = exists
          ? s.sidePanelTabs.map((t) => (t.id === tabId ? { ...t, title: resolvedTitle } : t))
          : [
              ...s.sidePanelTabs,
              {
                id: tabId,
                kind: "board" as const,
                title: resolvedTitle,
                hostId,
                workspaceId,
              },
            ];
        const shouldYield = !s.sidePanelOpen && s.isTightForDualSidebars && s.sidebarOpen;
        return {
          ...s,
          sidePanelOpen: true,
          overviewPopoverOpen: false,
          sidePanelTabs: nextTabs,
          activeSidePanelTabId: tabId,
          terminalFocused: false,
          sidebarOpen: shouldYield ? false : s.sidebarOpen,
          sidebarAutoYieldedForSidePanel: shouldYield ? true : s.sidebarAutoYieldedForSidePanel,
        };
      });
      void ctx.actions.fetchWorkspaceBoardAndEvents(hostId, workspaceId);
    },


    openPeerSplitTab(hostId: string, peerAgentId: string, title?: string): void {
      const detail = getState().hostDetails[hostId];
      const peer = detail?.agents.find((a) => a.agent_id === peerAgentId);
      const tabId = `tab:peer:${hostId}::${peerAgentId}`;
      const resolvedTitle =
        title ?? (peer?.handle ? `@${peer.handle}` : peer?.title ?? "Peer");
      setState((s) => {
        const exists = s.sidePanelTabs.some((t) => t.id === tabId);
        const nextTabs = exists
          ? s.sidePanelTabs.map((t) => (t.id === tabId ? { ...t, title: resolvedTitle } : t))
          : [
              ...s.sidePanelTabs,
              {
                id: tabId,
                kind: "peer" as const,
                title: resolvedTitle,
                hostId,
                peerAgentId,
                workspaceId: peer?.workspace_id,
                cwd: peer?.cwd,
              },
            ];
        const shouldYield = !s.sidePanelOpen && s.isTightForDualSidebars && s.sidebarOpen;
        return {
          ...s,
          sidePanelOpen: true,
          overviewPopoverOpen: false,
          sidePanelTabs: nextTabs,
          activeSidePanelTabId: tabId,
          terminalFocused: false,
          sidebarOpen: shouldYield ? false : s.sidebarOpen,
          sidebarAutoYieldedForSidePanel: shouldYield ? true : s.sidebarAutoYieldedForSidePanel,
        };
      });
      void orbitIpc.openAgentTimeline(hostId, peerAgentId);
      void ctx.actions.pullStageDelta(hostId, peerAgentId, true);
    },


    async fetchWorkspaceBoardAndEvents(hostId: string, workspaceId: string): Promise<void> {
      await Promise.all([
        orbitIpc.fetchWorkspaceBoard(hostId, workspaceId),
        orbitIpc.fetchWorkspaceEvents(hostId, workspaceId, 80),
      ]);
    },


    async updateWorkspaceBoard(
      hostId: string,
      workspaceId: string,
      content: string,
      section?: string | null,
    ): Promise<void> {
      await orbitIpc.updateWorkspaceBoard({
        hostId,
        workspaceId,
        content,
        section: section ?? null,
        updatedBy: "user",
      });
    },


    async updateWorkspaceGoal(hostId: string, workspaceId: string, goal: string): Promise<void> {
      await orbitIpc.updateWorkspaceGoal(hostId, workspaceId, goal);
      notifyToast("Mission goal updated");
    },


    async renameWorkspace(hostId: string, workspaceId: string, title: string): Promise<void> {
      const trimmed = title.trim();
      if (!trimmed) return;
      try {
        await orbitIpc.renameWorkspace(hostId, workspaceId, trimmed);
        ctx.actions.refreshSidebarView();
        notifyToast(t("store.workspaceRenamed", { title: trimmed }));
      } catch (err) {
        notifyToast(t("store.failedRenameGroup", { detail: String(err) }));
      }
    },


    async createMissionWorkspace(params: {
      hostId: string;
      sourceCwd: string;
      title: string;
      missionGoal?: string | null;
      isolation?: "local" | "worktree";
      worktreeBranch?: string | null;
    }): Promise<void> {
      const trimmed = params.title.trim();
      if (!trimmed) return;
      try {
        await orbitIpc.createMissionWorkspace({
          hostId: params.hostId,
          sourceCwd: params.sourceCwd,
          title: trimmed,
          missionGoal: params.missionGoal ?? null,
          isolation: params.isolation ?? "local",
          worktreeBranch: params.worktreeBranch ?? null,
        });
        ctx.actions.refreshSidebarView();
        notifyToast(t("store.workspaceCreated", { title: trimmed }));
      } catch (err) {
        notifyToast(t("store.failedCreateGroup", { detail: String(err) }));
      }
    },


    async archiveWorkspace(
      hostId: string,
      workspaceId: string,
      keepAgentsUnclassified = false,
      opts?: { emptyDelete?: boolean },
    ): Promise<void> {
      try {
        await orbitIpc.archiveWorkspace(hostId, workspaceId, keepAgentsUnclassified);
        ctx.actions.refreshSidebarView();
        notifyToast(
          opts?.emptyDelete
            ? t("store.workspaceDeleted")
            : keepAgentsUnclassified
              ? t("store.workspaceDisbanded")
              : t("store.workspaceArchived"),
        );
      } catch (err) {
        notifyToast(t("store.failedUpdateGroup", { detail: String(err) }));
      }
    },


    async moveAgentToWorkspace(
      hostId: string,
      agentId: string,
      targetWorkspaceId?: string | null,
      opts?: { newWorkspaceTitle?: string | null; makeUnclassified?: boolean },
    ): Promise<void> {
      try {
        await orbitIpc.moveAgentToWorkspace(hostId, agentId, targetWorkspaceId ?? null, opts);
        ctx.actions.refreshSidebarView();
        notifyToast(
          opts?.makeUnclassified
            ? t("store.movedToUnfiled")
            : targetWorkspaceId
              ? t("store.movedToWorkspace")
              : opts?.newWorkspaceTitle
                ? t("store.movedToNewWorkspace", { title: opts.newWorkspaceTitle })
                : t("store.movedToStandaloneWorkspace"),
        );
      } catch (err) {
        notifyToast(t("store.failedMoveConversation", { detail: String(err) }));
      }
    },


    async organizeProjectWorkspaces(
      hostId: string,
      projectRoot: string,
      agentIds: string[] = [],
      autoApply = true,
    ): Promise<void> {
      try {
        await orbitIpc.organizeProjectWorkspaces(hostId, projectRoot, agentIds, autoApply);
        notifyToast(
          agentIds.length === 1
            ? t("store.organizingSingleSession")
            : t("store.organizingUnfiledSessions"),
        );
      } catch (err) {
        notifyToast(t("store.failedOrganizeConversations", { detail: String(err) }));
      }
    },


    async summarizeAgent(hostId: string, agentId: string): Promise<void> {
      try {
        await orbitIpc.summarizeAgent(hostId, agentId);
        notifyToast(t("store.summarizingSession"));
      } catch (err) {
        notifyToast(t("store.failedSummarizeConversation", { detail: String(err) }));
      }
    },


    async updateAgentIdentity(params: {
      hostId: string;
      agentId: string;
      handle?: string | null;
      title?: string | null;
      taskSummary?: string | null;
      lockHandle?: boolean;
      lockTitle?: boolean;
    }): Promise<void> {
      try {
        await orbitIpc.updateAgentIdentity(params);
        ctx.actions.refreshSidebarView();
        if (params.handle) {
          notifyToast(
            t("store.updatedSessionTag", { handle: params.handle.trim().replace(/^@+/, "") }),
          );
        } else if (params.taskSummary !== undefined) {
          notifyToast(t("store.updatedSessionSummary"));
        }
      } catch (err) {
        notifyToast(t("store.failedUpdateConversation", { detail: String(err) }));
      }
    },


    async mergeWorkspaces(
      hostId: string,
      sourceWorkspaceId: string,
      targetWorkspaceId: string,
    ): Promise<void> {
      try {
        await orbitIpc.mergeWorkspaces(hostId, sourceWorkspaceId, targetWorkspaceId);
        ctx.actions.refreshSidebarView();
        notifyToast(t("store.workspaceMerged"));
      } catch (err) {
        notifyToast(t("store.failedMergeGroups", { detail: String(err) }));
      }
    },


    async dismissClusterSuggestion(hostId: string, agentId: string): Promise<void> {
      await orbitIpc.dismissClusterSuggestion(hostId, agentId);
    },


    async spawnWorkspacePeer(
      req: SpawnWorkspacePeerRequest,
      opts?: { openInSplit?: boolean },
    ): Promise<string> {
      const optimisticId = await orbitIpc.spawnWorkspacePeer(req);
      notifyToast(req.handle ? `Spawning @${req.handle}…` : "Spawning workspace peer…");
      if (opts?.openInSplit && optimisticId) {
        ctx.actions.openPeerSplitTab(
          req.host_id,
          optimisticId,
          req.handle ? `@${req.handle}` : undefined,
        );
      }
      return optimisticId;
    },
  };
}
