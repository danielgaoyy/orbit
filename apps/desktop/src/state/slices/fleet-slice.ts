import type { ComposerInlineDirective } from "@/lib/slash-commands";
import {
  DRAFTS_STORAGE_KEY,
  mergeMissingDraftSlices,
  parseAndValidatePersistedDrafts,
} from "@/lib/draft-persistence";
import {
  t,
} from "@/lib/i18n";
import {
  PINNED_MANUAL_ORDER_SCOPE,
  collectVisibleSidebarThreads,
} from "@/components/sidebar/sidebar-model";
import { mergeSubsetOrder } from "@/lib/drag/drag-constants";
import {
  defaultMimeForMedia,
} from "@/lib/media-utils";
import {
  getCachedPromptImage,
  lookupRecentSentTurnImage,
} from "@/lib/prompt-image-cache";
import {
  seedFallbackProviderCatalogs,
} from "@/lib/model-catalog";
import {
  sharedDisclosureStateStore,
} from "@/lib/disclosure-state";
import {
  isBareHomeDir,
  isLocalHostCard,
  matchesProjectDirectory,
  projectPathScopeKey,
} from "@/lib/projects";
import {
  SIDEBAR_DEFAULT_WIDTH_PX,
  clampSidebarPrefWidth,
} from "@/lib/workspace-layout";
import {
  type ClientThreadPrefSeed,
  type ExtractedCodeBlock,
  type GitReposInspectionView,
  type HostDetailView,
  type PromptBadge,
  PROVIDER_LIST,
  type ProviderKind,
  type SidebarProjectGroupView,
  isAgentBackgroundWaiting,
  isAgentForegroundRunning,
  orbitIpc,
} from "@/lib/orbit-ipc";
import {
  COMPACT_VIEWPORT_COLLAPSE_PX,
  COMPACT_VIEWPORT_EXPAND_PX,
  DUAL_SIDEBAR_RESTORE_PX,
  DUAL_SIDEBAR_YIELD_PX,
  PENDING_DRAFT_CREATE_TTL_MS,
  THREAD_PREF_MIGRATION_KEY,
  basenameFromPath,
  collectRemotelyArchivedAgentScopes,
  collectRemotelyArchivedDraftScopes,
  hasCompletedOnboarding,
  isAgentErrorState,
  omitRecordKeys,
  resolveAvailableDraftProvider,
  scheduleCoalescedGate,
  subagentArchiveKey,
  threadScopeKey,
  withBootstrapTimeout,
} from "../store-helpers";
import {
  EMPTY_SIDE_PANEL_UNSEEN_IDS,
  dropSidePanelScopes,
  promoteDraftSidePanelScope,
  rebindDraftSidePanelCwd,
  remapOptimisticSidePanelScopes,
  removeSubagentTabsFromScopes,
  resolveSidePanelScopeKey,
  switchSidePanelScope,
} from "../side-panel-scope";
import { selectPersonalAgentRef } from "../selectors/personal-agent-selectors";

function deepEqualJson(a: unknown, b: unknown): boolean {
  if (Object.is(a, b)) return true;
  if (a === null || b === null || typeof a !== "object" || typeof b !== "object") {
    return false;
  }
  const aIsArray = Array.isArray(a);
  if (aIsArray !== Array.isArray(b)) return false;
  if (aIsArray) {
    const arrA = a as unknown[];
    const arrB = b as unknown[];
    if (arrA.length !== arrB.length) return false;
    for (let i = 0; i < arrA.length; i += 1) {
      if (!deepEqualJson(arrA[i], arrB[i])) return false;
    }
    return true;
  }
  const recA = a as Record<string, unknown>;
  const recB = b as Record<string, unknown>;
  const keysA = Object.keys(recA);
  const keysB = Object.keys(recB);
  if (keysA.length !== keysB.length) return false;
  for (let i = 0; i < keysA.length; i += 1) {
    const k = keysA[i];
    if (!Object.prototype.hasOwnProperty.call(recB, k)) return false;
    if (!deepEqualJson(recA[k], recB[k])) return false;
  }
  return true;
}

function stabilizeRecord<T>(
  prev: Record<string, T> | undefined,
  next: Record<string, T> | undefined,
): Record<string, T> | undefined {
  if (prev === next) return prev;
  if (!prev || !next) return next;
  const nextKeys = Object.keys(next);
  const prevKeys = Object.keys(prev);
  let allSame = nextKeys.length === prevKeys.length;
  let reusedAny = false;
  const merged: Record<string, T> = {};
  for (let i = 0; i < nextKeys.length; i += 1) {
    const k = nextKeys[i];
    if (Object.prototype.hasOwnProperty.call(prev, k) && deepEqualJson(prev[k], next[k])) {
      merged[k] = prev[k];
      reusedAny = true;
    } else {
      merged[k] = next[k];
      allSame = false;
    }
  }
  if (allSame) return prev;
  return reusedAny ? merged : next;
}

/**
 * Structural-sharing stabilizer for JSON-deserialized `HostDetailView` snapshots.
 * Reuses `prev.<field>` references whenever a sub-field is structurally identical across
 * `host_changed` heartbeats so targeted selectors (`provider_catalogs`, `managed_skills`,
 * `profiles`, `acp_agents`, `artifacts`, `workspace_scripts`, `terminals`, `host.available_providers`,
 * etc.) preserve referential equality and avoid unnecessary React re-renders.
 */
export function stabilizeHostDetail(
  prev: HostDetailView | undefined,
  next: HostDetailView,
): HostDetailView {
  if (!prev || prev === next) return next;

  const host = deepEqualJson(prev.host, next.host)
    ? prev.host
    : deepEqualJson(prev.host.available_providers, next.host.available_providers)
      ? { ...next.host, available_providers: prev.host.available_providers }
      : next.host;

  const discovered_projects = deepEqualJson(prev.discovered_projects, next.discovered_projects)
    ? prev.discovered_projects
    : next.discovered_projects;
  const directory_listing = deepEqualJson(prev.directory_listing, next.directory_listing)
    ? prev.directory_listing
    : next.directory_listing;
  const models_by_provider = deepEqualJson(prev.models_by_provider, next.models_by_provider)
    ? prev.models_by_provider
    : next.models_by_provider;
  const workspaces = deepEqualJson(prev.workspaces, next.workspaces)
    ? prev.workspaces
    : next.workspaces;
  const agents = deepEqualJson(prev.agents, next.agents) ? prev.agents : next.agents;
  const replaced_optimistic_agents = deepEqualJson(
    prev.replaced_optimistic_agents,
    next.replaced_optimistic_agents,
  )
    ? prev.replaced_optimistic_agents
    : next.replaced_optimistic_agents;
  const quotas = deepEqualJson(prev.quotas, next.quotas) ? prev.quotas : next.quotas;
  const provider_catalogs = deepEqualJson(prev.provider_catalogs, next.provider_catalogs)
    ? prev.provider_catalogs
    : next.provider_catalogs;
  const terminals = deepEqualJson(prev.terminals, next.terminals) ? prev.terminals : next.terminals;
  const profiles = deepEqualJson(prev.profiles, next.profiles) ? prev.profiles : next.profiles;
  const schedules = deepEqualJson(prev.schedules, next.schedules) ? prev.schedules : next.schedules;
  const conditional_timers = deepEqualJson(prev.conditional_timers, next.conditional_timers)
    ? prev.conditional_timers
    : next.conditional_timers;
  const connected_hosts = deepEqualJson(prev.connected_hosts, next.connected_hosts)
    ? prev.connected_hosts
    : next.connected_hosts;
  const orchestration_config = deepEqualJson(prev.orchestration_config, next.orchestration_config)
    ? prev.orchestration_config
    : next.orchestration_config;
  const orchestration_skills = deepEqualJson(prev.orchestration_skills, next.orchestration_skills)
    ? prev.orchestration_skills
    : next.orchestration_skills;
  const managed_skills = deepEqualJson(prev.managed_skills, next.managed_skills)
    ? prev.managed_skills
    : next.managed_skills;
  const keep_awake_status = deepEqualJson(prev.keep_awake_status, next.keep_awake_status)
    ? prev.keep_awake_status
    : next.keep_awake_status;
  const workspace_scripts = stabilizeRecord(prev.workspace_scripts, next.workspace_scripts);
  const pending_permissions = deepEqualJson(prev.pending_permissions, next.pending_permissions)
    ? prev.pending_permissions
    : next.pending_permissions;
  const provider_subagents = stabilizeRecord(prev.provider_subagents, next.provider_subagents);
  const git_repos_by_cwd = stabilizeRecord(prev.git_repos_by_cwd, next.git_repos_by_cwd);
  const workspace_boards = stabilizeRecord(prev.workspace_boards, next.workspace_boards);
  const workspace_events = stabilizeRecord(prev.workspace_events, next.workspace_events);
  const workspace_organize_status = stabilizeRecord(
    prev.workspace_organize_status,
    next.workspace_organize_status,
  );
  const host_sync_state = deepEqualJson(prev.host_sync_state, next.host_sync_state)
    ? prev.host_sync_state
    : next.host_sync_state;
  const acp_agents = deepEqualJson(prev.acp_agents, next.acp_agents)
    ? prev.acp_agents
    : next.acp_agents;
  const artifacts = deepEqualJson(prev.artifacts, next.artifacts) ? prev.artifacts : next.artifacts;
  const goal_runs = deepEqualJson(prev.goal_runs, next.goal_runs) ? prev.goal_runs : next.goal_runs;
  const monitor_watches = deepEqualJson(prev.monitor_watches, next.monitor_watches)
    ? prev.monitor_watches
    : next.monitor_watches;
  const workflow_runs = deepEqualJson(prev.workflow_runs, next.workflow_runs)
    ? prev.workflow_runs
    : next.workflow_runs;

  if (
    prev.revision === next.revision &&
    prev.last_created_agent_id === next.last_created_agent_id &&
    prev.active_terminal_id === next.active_terminal_id &&
    prev.active_terminal_slot === next.active_terminal_slot &&
    host === prev.host &&
    discovered_projects === prev.discovered_projects &&
    directory_listing === prev.directory_listing &&
    models_by_provider === prev.models_by_provider &&
    workspaces === prev.workspaces &&
    agents === prev.agents &&
    replaced_optimistic_agents === prev.replaced_optimistic_agents &&
    quotas === prev.quotas &&
    provider_catalogs === prev.provider_catalogs &&
    terminals === prev.terminals &&
    profiles === prev.profiles &&
    schedules === prev.schedules &&
    conditional_timers === prev.conditional_timers &&
    connected_hosts === prev.connected_hosts &&
    orchestration_config === prev.orchestration_config &&
    orchestration_skills === prev.orchestration_skills &&
    managed_skills === prev.managed_skills &&
    keep_awake_status === prev.keep_awake_status &&
    workspace_scripts === prev.workspace_scripts &&
    pending_permissions === prev.pending_permissions &&
    provider_subagents === prev.provider_subagents &&
    git_repos_by_cwd === prev.git_repos_by_cwd &&
    workspace_boards === prev.workspace_boards &&
    workspace_events === prev.workspace_events &&
    workspace_organize_status === prev.workspace_organize_status &&
    host_sync_state === prev.host_sync_state &&
    acp_agents === prev.acp_agents &&
    artifacts === prev.artifacts &&
    goal_runs === prev.goal_runs &&
    monitor_watches === prev.monitor_watches &&
    workflow_runs === prev.workflow_runs
  ) {
    return prev;
  }

  return {
    ...next,
    host,
    discovered_projects,
    directory_listing,
    models_by_provider,
    workspaces,
    agents,
    replaced_optimistic_agents,
    quotas,
    provider_catalogs,
    terminals,
    profiles,
    schedules,
    conditional_timers,
    connected_hosts,
    orchestration_config,
    orchestration_skills,
    managed_skills,
    keep_awake_status,
    workspace_scripts,
    pending_permissions,
    provider_subagents,
    git_repos_by_cwd,
    workspace_boards,
    workspace_events,
    workspace_organize_status,
    host_sync_state,
    acp_agents,
    artifacts,
    goal_runs,
    monitor_watches,
    workflow_runs,
  };
}
import type { OrbitStoreContext } from "../store-runtime";
import type {
  ComposerFocusOptions,
  ContextMentionItem,
  SidebarProjectSort,
  SidebarThreadFilter,
  SlashDirective,
  TerminalTabSession,
  UndoArchiveEntry,
  WorktreeHandoffDialogTarget,
} from "../store-types";

export function createFleetSlice(ctx: OrbitStoreContext) {
  const {
    bumpFileLoadSeq,
    composerFocusListeners,
    detachedTerminalIds,
    dispatchDesktopNotification,
    draftPersistence,
    drainPromptQueueIfIdle,
    flushPendingSeenMarks,
    getState,
    hostSyncGates,
    initialPrefs,
    lastReportedSeenSeq,
    markThreadSeenInCore,
    maybeAutoSyncOutdatedHosts,
    notifiedTerminalGoals,
    notifyToast,
    scheduleAutoRefreshGit,
    scheduleSavePrefs,
    setState,
    sidebarSyncGate,
    stagePullGates,
    subscribeCoreEvents,
    syncAttentionIndicators,
    trackNotifiedTerminalGoal,
  } = ctx;

  let copyBlocksReqSeq = 0;
  const inFlightForks = new Set<string>();
  let onboardingAutoPromptChecked = false;

  return {

    async init(): Promise<void> {
      if (ctx.isInitialized()) return;
      ctx.setInitialized(true);

      // 1. One-time migration of legacy localStorage pin/seen prefs into Rust SQLite
      if (typeof window !== "undefined") {
        try {
          if (window.localStorage.getItem(THREAD_PREF_MIGRATION_KEY) !== "1") {
            const seedsByKey = new Map<string, ClientThreadPrefSeed>();
            const pinnedKeys = initialPrefs.pinnedThreadKeys ?? [];
            const seenMap = initialPrefs.seenAgentSeqs ?? {};
            const nowMs = Date.now();
            pinnedKeys.forEach((rawKey, idx) => {
              const sep = rawKey.indexOf("::");
              if (sep <= 0) return;
              const hostId = rawKey.slice(0, sep);
              const agentId = rawKey.slice(sep + 2);
              seedsByKey.set(rawKey, {
                host_id: hostId,
                agent_id: agentId,
                seen_seq: seenMap[rawKey] ?? 0,
                is_pinned: true,
                pinned_order: idx + 1,
                updated_at_ms: nowMs,
              });
            });
            for (const [rawKey, seq] of Object.entries(seenMap)) {
              if (seedsByKey.has(rawKey)) continue;
              const sep = rawKey.indexOf("::");
              if (sep <= 0) continue;
              seedsByKey.set(rawKey, {
                host_id: rawKey.slice(0, sep),
                agent_id: rawKey.slice(sep + 2),
                seen_seq: seq,
                is_pinned: false,
                pinned_order: 0,
                updated_at_ms: nowMs,
              });
            }
            if (seedsByKey.size > 0) {
              await orbitIpc.seedThreadPrefs(Array.from(seedsByKey.values())).catch(() => undefined);
            }
            window.localStorage.setItem(THREAD_PREF_MIGRATION_KEY, "1");
            scheduleSavePrefs();
          }
        } catch {
          // Ignore storage access errors
        }
      }

      // 2. Non-blocking secondary metadata bootstrap (never blocks or aborts fleet hydration)
      void (async () => {
        const [
          editors,
          sshList,
          version,
          fallbackCatalogs,
          slashCommands,
          cloudSyncStatus,
          sqliteDraftJson,
          primaryRouterSel,
        ] = await Promise.all([
          orbitIpc.listExternalEditors().catch(() => []),
          orbitIpc.listSshHosts().catch(() => []),
          orbitIpc.getAppVersion().catch(() => null),
          orbitIpc.getFallbackProviderCatalogs().catch(() => []),
          orbitIpc.getSlashCommands("desktop").catch(() => []),
          orbitIpc.getCloudSyncStatus().catch(() => null),
          orbitIpc.getComposerDraft(DRAFTS_STORAGE_KEY).catch(() => null),
          orbitIpc.getPrimaryRouterSelection().catch(() => null),
        ]);
        if (sqliteDraftJson) {
          const parsedSqliteDrafts = parseAndValidatePersistedDrafts(sqliteDraftJson, {
            excludeScopes: getState().archivedThreads,
          });
          const mergedDrafts = mergeMissingDraftSlices(getState(), parsedSqliteDrafts);
          if (mergedDrafts) {
            ctx.draftPersistence.restoreUpdatedAtByScope(mergedDrafts.updatedAtByScope);
            setState((s) => ({
              ...s,
              ...(mergedDrafts.composerDrafts ? { composerDrafts: mergedDrafts.composerDrafts } : {}),
              ...(mergedDrafts.pastedAttachments ? { pastedAttachments: mergedDrafts.pastedAttachments } : {}),
              ...(mergedDrafts.diffQuotes ? { diffQuotes: mergedDrafts.diffQuotes } : {}),
              ...(mergedDrafts.contextItems ? { contextItems: mergedDrafts.contextItems } : {}),
              ...(mergedDrafts.inlineMentions ? { inlineMentions: mergedDrafts.inlineMentions } : {}),
              ...(mergedDrafts.slashDirectives ? { slashDirectives: mergedDrafts.slashDirectives } : {}),
              ...(mergedDrafts.sideQuestionDrafts ? { sideQuestionDrafts: mergedDrafts.sideQuestionDrafts } : {}),
            }));
            ctx.draftPersistence.markDirty(mergedDrafts.recoveredScopes);
            ctx.emitComposerDraftChange();
            ctx.emitDraftPresenceChange();
          }
        }
        seedFallbackProviderCatalogs(fallbackCatalogs);
        setState((s) => ({
          ...s,
          externalEditors: editors,
          sshHosts: sshList,
          appVersion: version,
          slashCommands,
          cloudSyncStatus,
          primaryRouterHostId: primaryRouterSel?.host_id ?? s.primaryRouterHostId,
        }));
        if (cloudSyncStatus?.state === "ready") {
          void ctx.actions.hydrateSyncedPreferencesFromCore();
        }
        maybeAutoSyncOutdatedHosts();

        // Sanitize persisted draft models against Rust catalog & retired-slug table
        const providers: ProviderKind[] = PROVIDER_LIST.map((p) => p.id);
        const sanitizedEntries = await Promise.all(
          providers.map(async (provider) => {
            const current = getState().draftModels[provider] ?? null;
            if (!current) return [provider, null] as const;
            const resolved = await orbitIpc
              .resolveModelSelection({
                host_id: null,
                provider,
                desired_model: current,
                desired_effort: getState().draftEffort,
              })
              .catch(() => null);
            return [provider, resolved ? resolved.selected_model_id : current] as const;
          }),
        );
        const nextDraftModels = { ...getState().draftModels };
        let modelsChanged = false;
        for (const [provider, nextModel] of sanitizedEntries) {
          if (nextDraftModels[provider] !== nextModel) {
            nextDraftModels[provider] = nextModel;
            modelsChanged = true;
          }
        }
        if (modelsChanged) {
          setState((s) => ({ ...s, draftModels: nextDraftModels }), true);
        }
      })();

      // 3. Highest priority: subscribe to live core events and hydrate fleet/local host immediately
      const subscribePromise = subscribeCoreEvents();

      try {
        await withBootstrapTimeout(
          ctx.actions.reloadFleetAndHosts(true),
          "Workspace hydration",
        );
      } catch (err) {
        setState((s) =>
          s.fleetHydrated
            ? s
            : {
                ...s,
                bootstrapError: err instanceof Error ? err.message : String(err),
              },
        );
      }
      ctx.actions.refreshSidebarView();
      await subscribePromise;
    },


    async retryBootstrap(): Promise<void> {
      setState((s) => ({ ...s, bootstrapError: null }));
      const subscribePromise = subscribeCoreEvents();
      try {
        await withBootstrapTimeout(
          ctx.actions.reloadFleetAndHosts(true),
          "Workspace hydration",
        );
      } catch (err) {
        setState((s) =>
          s.fleetHydrated
            ? s
            : {
                ...s,
                bootstrapError: err instanceof Error ? err.message : String(err),
              },
        );
      }
      ctx.actions.refreshSidebarView();
      await subscribePromise;
    },


    refreshSidebarView(): void {
      scheduleCoalescedGate(sidebarSyncGate, async () => {
        const view = await orbitIpc.getFleetSidebar({
          sort: getState().sidebarProjectSort,
          filter: getState().sidebarThreadFilter,
          query: "",
          active_host_id: getState().selectedHostId,
          active_agent_id: getState().isDraftNewSession ? null : getState().selectedAgentId,
          active_cwd: getState().selectedWorkspaceCwd || null,
        });
        setState((s) => ({ ...s, sidebarView: view }));
      });
    },


    async reloadFleetOnly(): Promise<void> {
      const snap = await orbitIpc.getFleetOverview();
      setState((s) => {
        const nextHostId = s.selectedHostId ?? snap.hosts[0]?.host_id ?? null;
        const nextScopeKey = resolveSidePanelScopeKey({
          selectedHostId: nextHostId,
          selectedAgentId: s.selectedAgentId,
          isDraftNewSession: s.isDraftNewSession,
          hostDetails: s.hostDetails,
        });
        const scoped = switchSidePanelScope(s, nextScopeKey);
        return {
          ...scoped,
          fleet: snap,
          selectedHostId: nextHostId,
        };
      });
      syncAttentionIndicators(getState().fleet, getState().hostDetails);
      ctx.actions.refreshSidebarView();
    },


    setBrowserOffline(offline: boolean): void {
      const prev = getState().isBrowserOffline;
      if (prev === offline) return;
      setState((s) => ({ ...s, isBrowserOffline: offline }));
      if (!offline) {
        notifyToast("Back online");
        for (const h of getState().fleet.hosts) {
          if (h.connection_state === "disconnected") {
            void ctx.actions.reconnectHost(h.host_id);
          }
        }
      }
    },


    dismissVersionMismatch(hostId: string, daemonVersion: string): void {
      setState((s) => ({
        ...s,
        dismissedVersionMismatchHosts: {
          ...s.dismissedVersionMismatchHosts,
          [hostId]: daemonVersion,
        },
      }));
    },


    async reconnectHost(hostId: string): Promise<void> {
      try {
        await orbitIpc.updateHostConfig({ hostId, reconnect: true });
        await ctx.actions.reloadFleetAndHosts(false);
      } catch (err) {
        notifyToast(`Reconnect failed: ${String(err)}`);
      }
    },

    async applyRelayToAllHosts(rawRelayUrl: string): Promise<number> {
      const count = await orbitIpc.applyRelayToAllHosts(rawRelayUrl);
      await ctx.actions.reloadFleetAndHosts(false);
      return count;
    },


    async reloadFleetAndHosts(initialBootstrap = false): Promise<void> {
      const snap = await orbitIpc.getFleetOverview();

      const detailEntries = await Promise.all(
        snap.hosts.map(async (h) => {
          const d = await orbitIpc.getHostDetail(h.host_id);
          return [h.host_id, d] as const;
        }),
      );

      const rawDetails: Record<string, HostDetailView> = {};
      for (const [hid, d] of detailEntries) {
        if (d) {
          rawDetails[hid] = d;
          // Fleet reload already fetched fresh host detail for `hid`; cancel redundant
          // same-frame per-host sync scheduled by a paired `HostChanged` notification.
          const hGate = hostSyncGates.get(hid);
          if (hGate && !hGate.inFlight) {
            if (hGate.rafId !== null && typeof window !== "undefined") {
              window.cancelAnimationFrame(hGate.rafId);
              hGate.rafId = null;
            }
            hGate.queued = false;
          }
        }
      }

      const prevSnap = getState();
      const previewTargetHostId = prevSnap.selectedHostId ?? snap.hosts[0]?.host_id ?? null;
      const previewActiveHostId =
        previewTargetHostId && snap.hosts.some((h) => h.host_id === previewTargetHostId)
          ? previewTargetHostId
          : (snap.hosts[0]?.host_id ?? null);
      const previewNextDraftProvider = resolveAvailableDraftProvider(
        previewActiveHostId ? rawDetails[previewActiveHostId] : undefined,
        prevSnap.draftProvider,
      );
      const shouldPersistPrefs = previewNextDraftProvider !== prevSnap.draftProvider;

      setState((s) => {
        const nextDetails: Record<string, HostDetailView> = {};
        const prevDetailKeys = Object.keys(s.hostDetails);
        const rawDetailKeys = Object.keys(rawDetails);
        let allDetailsSame = prevDetailKeys.length === rawDetailKeys.length;
        for (const hid of rawDetailKeys) {
          const prevDetail = s.hostDetails[hid];
          const stabilized = stabilizeHostDetail(prevDetail, rawDetails[hid]);
          nextDetails[hid] = stabilized;
          if (stabilized !== prevDetail) {
            allDetailsSame = false;
          }
        }
        const resolvedDetails = allDetailsSame ? s.hostDetails : nextDetails;

        const targetHostId = s.selectedHostId ?? snap.hosts[0]?.host_id ?? null;
        const activeHostId =
          targetHostId && snap.hosts.some((h) => h.host_id === targetHostId)
            ? targetHostId
            : (snap.hosts[0]?.host_id ?? null);
        const activeDetail = activeHostId ? resolvedDetails[activeHostId] : undefined;

        let nextAgentId = s.isDraftNewSession ? null : s.selectedAgentId;
        let nextDraft = s.isDraftNewSession;
        if (!activeHostId) {
          nextAgentId = null;
          nextDraft = true;
        } else if (!s.isDraftNewSession && activeDetail) {
          const stillExists =
            nextAgentId &&
            !s.archivedThreads[threadScopeKey(activeHostId, nextAgentId)] &&
            activeDetail.agents.some((a) => a.agent_id === nextAgentId);
          if (!stillExists) {
            const firstVisible = activeDetail.agents.find(
              (a) =>
                !a.parent_agent_id &&
                !a.background &&
                !a.archived_at_rfc3339 &&
                !s.archivedThreads[threadScopeKey(activeHostId, a.agent_id)],
            );
            nextAgentId = firstVisible?.agent_id ?? null;
            nextDraft = nextAgentId === null;
          }
        }

        const activeAgent =
          !nextDraft && nextAgentId
            ? activeDetail?.agents.find((a) => a.agent_id === nextAgentId)
            : undefined;
        const nextCwd = !activeHostId
          ? "~"
          : activeAgent?.cwd ||
            s.selectedWorkspaceCwd ||
            activeDetail?.workspaces[0]?.cwd ||
            activeDetail?.discovered_projects[0]?.path ||
            "~";

        const nextDraftProvider = resolveAvailableDraftProvider(activeDetail, s.draftProvider);

        const activeScopeKey =
          !nextDraft && activeHostId && nextAgentId
            ? threadScopeKey(activeHostId, nextAgentId)
            : null;
        const remotelyArchivedDraftKeys = collectRemotelyArchivedDraftScopes(
          s,
          Object.entries(resolvedDetails),
          activeScopeKey,
        );
        const remotelyArchivedAgents = collectRemotelyArchivedAgentScopes(
          Object.entries(resolvedDetails),
          activeScopeKey,
        );
        let scoped =
          remotelyArchivedAgents.keys.size > 0
            ? dropSidePanelScopes(s, remotelyArchivedAgents.keys, remotelyArchivedAgents.agents)
            : s;
        const nextScopeKey = resolveSidePanelScopeKey({
          selectedHostId: activeHostId,
          selectedAgentId: nextAgentId,
          isDraftNewSession: nextDraft,
          hostDetails: resolvedDetails,
          archivedThreads: s.archivedThreads,
        });
        scoped = switchSidePanelScope(scoped, nextScopeKey);

        return {
          ...scoped,
          fleet: snap,
          fleetHydrated: true,
          bootstrapError: null,
          hostDetails: resolvedDetails,
          selectedHostId: activeHostId,
          selectedAgentId: nextAgentId,
          selectedWorkspaceCwd: nextCwd,
          isDraftNewSession: nextDraft,
          draftProvider: nextDraftProvider,
          ...(remotelyArchivedDraftKeys.size > 0
            ? {
                composerDrafts: omitRecordKeys(s.composerDrafts, remotelyArchivedDraftKeys),
                slashDirectives: omitRecordKeys(s.slashDirectives, remotelyArchivedDraftKeys),
                inlineMentions: omitRecordKeys(s.inlineMentions, remotelyArchivedDraftKeys),
                pastedAttachments: omitRecordKeys(s.pastedAttachments, remotelyArchivedDraftKeys),
                diffQuotes: omitRecordKeys(s.diffQuotes, remotelyArchivedDraftKeys),
                sideQuestionDrafts: omitRecordKeys(s.sideQuestionDrafts, remotelyArchivedDraftKeys),
              }
            : {}),
        };
      }, shouldPersistPrefs);

      syncAttentionIndicators(getState().fleet, getState().hostDetails);
      ctx.actions.refreshSidebarView();

      if (initialBootstrap && !onboardingAutoPromptChecked) {
        onboardingAutoPromptChecked = true;
        if (!hasCompletedOnboarding() && getState().activeView === "workspace") {
          const hasAnyActiveConversation =
            getState().fleet.hosts.some((h) => h.agent_count > 0) ||
            Object.values(getState().hostDetails).some((detail) =>
              detail.agents.some((ag) => !ag.archived_at_rfc3339),
            );
          if (!hasAnyActiveConversation) {
            setState((s) => ({ ...s, onboardingOpen: true }));
          }
        }
      }

      const afterState = getState();
      if (afterState.selectedHostId) {
        if (initialBootstrap) {
          void orbitIpc.refreshQuotas(afterState.selectedHostId);
          if (afterState.selectedWorkspaceCwd && afterState.selectedWorkspaceCwd !== "~") {
            void ctx.actions.refreshGit(
              afterState.selectedHostId,
              afterState.selectedWorkspaceCwd,
              null,
              afterState.diffStaged,
            );
          }
        }
        if (afterState.selectedAgentId && !afterState.isDraftNewSession) {
          const tlKey = `${afterState.selectedHostId}::${afterState.selectedAgentId}`;
          const activeAg = afterState.hostDetails[afterState.selectedHostId]?.agents.find(
            (a) => a.agent_id === afterState.selectedAgentId,
          );
          if (activeAg) {
            markThreadSeenInCore(afterState.selectedHostId, afterState.selectedAgentId, activeAg.latest_seq);
          }
          if (initialBootstrap || !afterState.stages[tlKey]) {
            void orbitIpc.openAgentTimeline(afterState.selectedHostId, afterState.selectedAgentId);
            void orbitIpc.listProviderSubagents(afterState.selectedHostId, afterState.selectedAgentId);
            void ctx.actions.pullStageDelta(afterState.selectedHostId, afterState.selectedAgentId, true);
          }
        }
      }
      maybeAutoSyncOutdatedHosts();
    },


    async reloadHostDetail(hostId: string, includeFleetOverview = false): Promise<void> {
      const [detail, overview] = includeFleetOverview
        ? await Promise.all([orbitIpc.getHostDetail(hostId), orbitIpc.getFleetOverview()])
        : [await orbitIpc.getHostDetail(hostId), null];
      if (!detail) return;

      const curSnap = getState();
      const prevHostDetail = curSnap.hostDetails[hostId];
      const prevAgents = prevHostDetail?.agents ?? [];

      let settledGitRefreshCwd: string | null = null;
      if (curSnap.selectedHostId === hostId && !curSnap.isDraftNewSession && curSnap.selectedAgentId) {
        const selId = curSnap.selectedAgentId;
        const prevActive = prevAgents.find((a) => a.agent_id === selId);
        const nextActive = detail.agents.find((a) => a.agent_id === selId);
        const activeTurnSettled =
          isAgentForegroundRunning(prevActive) && !isAgentForegroundRunning(nextActive);
        const childSubagentSettled = detail.agents.some((nextAg) => {
          if (nextAg.parent_agent_id !== selId) return false;
          const prevAg = prevAgents.find((p) => p.agent_id === nextAg.agent_id);
          return isAgentForegroundRunning(prevAg) && !isAgentForegroundRunning(nextAg);
        });
        const prevProviderSubs = prevHostDetail?.provider_subagents?.[selId] ?? [];
        const nextProviderSubs = detail.provider_subagents?.[selId] ?? [];
        const providerSubagentSettled = nextProviderSubs.some((nextPs) => {
          if (
            nextPs.status !== "completed" &&
            nextPs.status !== "failed" &&
            nextPs.status !== "canceled" &&
            nextPs.status !== "stopped"
          ) {
            return false;
          }
          const prevPs = prevProviderSubs.find((p) => p.subagent_id === nextPs.subagent_id);
          return !prevPs || prevPs.status === "running";
        });
        if (activeTurnSettled || childSubagentSettled || providerSubagentSettled) {
          settledGitRefreshCwd = nextActive?.cwd || curSnap.selectedWorkspaceCwd || null;
        }
      }
      const pendingDraft = ctx.getPendingDraftCreate();
      if (
        pendingDraft &&
        Date.now() - pendingDraft.startedAt > PENDING_DRAFT_CREATE_TTL_MS
      ) {
        ctx.setPendingDraftCreate(null);
      }
      const freshPendingDraft = ctx.getPendingDraftCreate();
      const awaitingDraftForHost =
        freshPendingDraft !== null && freshPendingDraft.hostId === hostId;
      const newestCreated =
        curSnap.selectedHostId === hostId && curSnap.isDraftNewSession && awaitingDraftForHost
          ? (freshPendingDraft?.optimisticAgentId
              ? detail.agents.find((a) => a.agent_id === freshPendingDraft?.optimisticAgentId)
              : undefined) ??
            detail.agents.find(
              (a) =>
                !a.parent_agent_id &&
                !a.background &&
                !prevAgents.some((p) => p.agent_id === a.agent_id),
            )
          : undefined;
      if (newestCreated) {
        ctx.setPendingDraftCreate(null);
      }

      for (const nextAg of detail.agents) {
        const prevAg = prevAgents.find((p) => p.agent_id === nextAg.agent_id);
        if (
          getState().notifyOnTurnComplete &&
          isAgentForegroundRunning(prevAg) &&
          !isAgentForegroundRunning(nextAg) &&
          !nextAg.is_awaiting_permission &&
          !isAgentErrorState(nextAg)
        ) {
          dispatchDesktopNotification({
            title: `Orbit · ${detail.host.display_name}`,
            body: `Completed: ${nextAg.title}`,
            hostId,
            agentId: nextAg.agent_id,
            tag: `orbit-done-${hostId}-${nextAg.agent_id}`,
          });
        } else if (
          getState().notifyOnAgentError &&
          prevAg !== undefined &&
          !isAgentErrorState(prevAg) &&
          isAgentErrorState(nextAg)
        ) {
          dispatchDesktopNotification({
            title: `Orbit · ${detail.host.display_name}`,
            body: `Agent error: ${nextAg.title}`,
            hostId,
            agentId: nextAg.agent_id,
            tag: `orbit-error-${hostId}-${nextAg.agent_id}`,
            tone: "error",
          });
        }
      }

      const prevGoals = getState().hostDetails[hostId]?.goal_runs ?? [];
      const nextGoals = detail.goal_runs ?? [];
      const liveGoalKeysForHost = new Set(nextGoals.map((g) => `${hostId}::${g.goal_id}`));
      const hostGoalPrefix = `${hostId}::`;
      for (const existingKey of Array.from(notifiedTerminalGoals)) {
        if (existingKey.startsWith(hostGoalPrefix) && !liveGoalKeysForHost.has(existingKey)) {
          notifiedTerminalGoals.delete(existingKey);
        }
      }
      for (const nextGoal of nextGoals) {
        const gKey = `${hostId}::${nextGoal.goal_id}`;
        const isTerminal =
          nextGoal.status === "succeeded" ||
          nextGoal.status === "failed" ||
          nextGoal.status === "canceled";
        if (!isTerminal) {
          notifiedTerminalGoals.delete(gKey);
          continue;
        }
        const prevGoal = prevGoals.find((g) => g.goal_id === nextGoal.goal_id);
        const wasActive =
          prevGoal !== undefined &&
          (prevGoal.status === "running" ||
            prevGoal.status === "verifying" ||
            prevGoal.status === "paused");
        if (getState().notifyOnGoalFinished && wasActive && !notifiedTerminalGoals.has(gKey)) {
          trackNotifiedTerminalGoal(gKey);
          dispatchDesktopNotification({
            title: `Orbit · ${detail.host.display_name}`,
            body: `Goal ${nextGoal.status}: ${nextGoal.objective}`,
            hostId,
            agentId: nextGoal.agent_id,
            tag: `orbit-goal-${hostId}-${nextGoal.goal_id}`,
          });
        }
      }

      let viewedAgentLatestSeq: number | null = null;
      let viewedAgentIdToMark: string | null = null;
      let viewedAgentWasUnread = false;
      let newlyAttachedTerminal: { tabKey: string; terminalId: string } | null = null;

      for (const ag of detail.agents) {
        if (ag.is_unread) {
          lastReportedSeenSeq.delete(threadScopeKey(hostId, ag.agent_id));
        }
      }

      const prevSyncUpdatedAt = getState().hostDetails[hostId]?.host_sync_state?.updated_at_ms ?? 0;
      const nextSync = detail.host_sync_state ?? null;
      const syncedSortMode =
        nextSync &&
        nextSync.updated_at_ms > prevSyncUpdatedAt &&
        (nextSync.project_sort_mode === "recent" ||
          nextSync.project_sort_mode === "alphabetical" ||
          nextSync.project_sort_mode === "manual")
          ? (nextSync.project_sort_mode as SidebarProjectSort)
          : null;

      setState((s) => {
        const prevDetail = s.hostDetails[hostId];
        const stabilizedDetail = stabilizeHostDetail(prevDetail, detail);
        const nextDetails =
          stabilizedDetail === prevDetail
            ? s.hostDetails
            : { ...s.hostDetails, [hostId]: stabilizedDetail };
        let nextGitReposCache = s.gitReposCache;
        if (stabilizedDetail.git_repos_by_cwd) {
          let clonedGitRepos = false;
          for (const [cwdKey, insp] of Object.entries(stabilizedDetail.git_repos_by_cwd)) {
            const fullKey = `${hostId}::${cwdKey}`;
            if (!deepEqualJson(s.gitReposCache[fullKey], insp)) {
              if (!clonedGitRepos) {
                nextGitReposCache = { ...s.gitReposCache };
                clonedGitRepos = true;
              }
              nextGitReposCache[fullKey] = insp;
            }
          }
        }
        const replacedMap = detail.replaced_optimistic_agents ?? {};
        const resolveReplacedAgentId = (id: string | null): string | null => {
          if (!id) return null;
          let cur = id;
          for (let i = 0; i < 4 && replacedMap[cur]; i += 1) {
            cur = replacedMap[cur];
          }
          return cur;
        };
        const remapScopedRecord = <T>(rec: Record<string, T>): Record<string, T> => {
          let changed = false;
          const nextRec = { ...rec };
          for (const [optId, realId] of Object.entries(replacedMap)) {
            const oldKey = `${hostId}::${optId}`;
            const newKey = `${hostId}::${realId}`;
            if (oldKey in nextRec) {
              if (!(newKey in nextRec)) {
                nextRec[newKey] = nextRec[oldKey];
              }
              delete nextRec[oldKey];
              changed = true;
            }
          }
          return changed ? nextRec : rec;
        };

        let nextAgentId = s.selectedHostId === hostId ? resolveReplacedAgentId(s.selectedAgentId) : s.selectedAgentId;
        if (s.selectedHostId === hostId && s.selectedAgentId && nextAgentId && nextAgentId !== s.selectedAgentId) {
          sharedDisclosureStateStore.migrateAgent(s.selectedAgentId, nextAgentId);
        }
        let nextDraft = s.isDraftNewSession;
        let nextCwd = s.selectedWorkspaceCwd;

        if (newestCreated) {
          nextAgentId = newestCreated.agent_id;
          nextDraft = false;
          nextCwd = newestCreated.cwd;
        } else if (s.selectedHostId === hostId && !s.isDraftNewSession) {
          if (
            !nextAgentId ||
            s.archivedThreads[threadScopeKey(hostId, nextAgentId)] ||
            !detail.agents.some((a) => a.agent_id === nextAgentId)
          ) {
            const prevOptId =
              nextAgentId &&
              nextAgentId.startsWith("opt_") &&
              !detail.agents.some((a) => a.agent_id === nextAgentId)
                ? nextAgentId
                : null;
            const firstVisible = detail.agents.find(
              (a) =>
                !a.parent_agent_id &&
                !a.background &&
                !a.archived_at_rfc3339 &&
                !s.archivedThreads[threadScopeKey(hostId, a.agent_id)],
            );
            nextAgentId = firstVisible?.agent_id ?? null;
            nextDraft = nextAgentId === null;
            if (prevOptId && nextAgentId && !nextAgentId.startsWith("opt_")) {
              sharedDisclosureStateStore.migrateAgent(prevOptId, nextAgentId);
            }
          }
        }

        if (s.workspacePanel === "conversation" && !nextDraft && s.selectedHostId === hostId && nextAgentId) {
          const viewed = detail.agents.find((a) => a.agent_id === nextAgentId);
          if (viewed) {
            viewedAgentIdToMark = viewed.agent_id;
            viewedAgentLatestSeq = viewed.latest_seq;
            viewedAgentWasUnread = Boolean(viewed.is_unread || viewed.attention === "manual_unread");
          }
        }

        // Prune exited terminals from the detached suppression set for this host
        const livePrefix = `${hostId}::`;
        const liveTermIds = new Set(detail.terminals.map((t) => t.terminal_id));
        for (const detachedKey of detachedTerminalIds) {
          if (detachedKey.startsWith(livePrefix)) {
            const tid = detachedKey.slice(livePrefix.length);
            if (!liveTermIds.has(tid)) {
              detachedTerminalIds.delete(detachedKey);
            }
          }
        }

        // Sync attached terminal tab metadata only when the server terminal is still live
        let nextTabs = s.terminalTabs;
        let nextActiveTab = s.activeTerminalTabKey;
        if (
          detail.active_terminal_id &&
          !detachedTerminalIds.has(`${hostId}::${detail.active_terminal_id}`)
        ) {
          const termMeta = detail.terminals.find((t) => t.terminal_id === detail.active_terminal_id);
          if (termMeta) {
            const tabKey = `${hostId}::slot-${detail.active_terminal_slot}`;
            const existingIdx = nextTabs.findIndex((t) => t.key === tabKey);
            const entry: TerminalTabSession = {
              key: tabKey,
              hostId,
              terminalId: detail.active_terminal_id,
              slot: detail.active_terminal_slot,
              cwd: termMeta.cwd || nextCwd,
              title: termMeta.title || "shell",
            };
            if (existingIdx >= 0) {
              const existing = nextTabs[existingIdx];
              if (
                existing.terminalId !== entry.terminalId ||
                existing.slot !== entry.slot ||
                existing.cwd !== entry.cwd ||
                existing.title !== entry.title
              ) {
                nextTabs = [...nextTabs];
                nextTabs[existingIdx] = entry;
              }
            } else {
              nextTabs = [...nextTabs, entry];
              nextActiveTab = tabKey;
              newlyAttachedTerminal = { tabKey, terminalId: detail.active_terminal_id };
            }
          }
        }

        const nextDraftProvider =
          s.selectedHostId === hostId
            ? resolveAvailableDraftProvider(detail, s.draftProvider)
            : s.draftProvider;

        const activeScopeKey =
          !nextDraft && s.selectedHostId && nextAgentId
            ? threadScopeKey(s.selectedHostId, nextAgentId)
            : null;
        const remotelyArchivedDraftKeys = collectRemotelyArchivedDraftScopes(
          s,
          [[hostId, detail]],
          activeScopeKey,
        );
        const remotelyArchivedAgents = collectRemotelyArchivedAgentScopes(
          [[hostId, detail]],
          activeScopeKey,
        );
        const remappedStages = remapScopedRecord(s.stages);
        const remappedComposerDrafts = remapScopedRecord(s.composerDrafts);
        const remappedSlashDirectives = remapScopedRecord(s.slashDirectives);
        const remappedInlineMentions = remapScopedRecord(s.inlineMentions);
        const remappedContextItems = remapScopedRecord(s.contextItems);
        const remappedPastedAttachments = remapScopedRecord(s.pastedAttachments);
        const remappedDiffQuotes = remapScopedRecord(s.diffQuotes);
        const remappedSideQuestionsByScope = remapScopedRecord(s.sideQuestionsByScope);
        const remappedSideQuestionDrafts = remapScopedRecord(s.sideQuestionDrafts);

        const nextHostId = overview
          ? (s.selectedHostId ?? overview.hosts[0]?.host_id ?? null)
          : s.selectedHostId;
        let scoped =
          Object.keys(replacedMap).length > 0
            ? remapOptimisticSidePanelScopes(s, hostId, replacedMap)
            : s;
        if (
          remappedSideQuestionsByScope !== s.sideQuestionsByScope ||
          remappedSideQuestionDrafts !== s.sideQuestionDrafts
        ) {
          scoped = {
            ...scoped,
            sideQuestionsByScope: remappedSideQuestionsByScope,
            sideQuestionDrafts: remappedSideQuestionDrafts,
          };
        }
        if (remotelyArchivedAgents.keys.size > 0) {
          scoped = dropSidePanelScopes(
            scoped,
            remotelyArchivedAgents.keys,
            remotelyArchivedAgents.agents,
          );
        }
        if (newestCreated) {
          scoped = promoteDraftSidePanelScope(
            scoped,
            hostId,
            newestCreated.agent_id,
            newestCreated.cwd,
          );
        } else {
          const nextScopeKey = resolveSidePanelScopeKey({
            selectedHostId: nextHostId,
            selectedAgentId: nextAgentId,
            isDraftNewSession: nextDraft,
            hostDetails: nextDetails,
            archivedThreads: s.archivedThreads,
          });
          scoped = switchSidePanelScope(scoped, nextScopeKey);
        }

        return {
          ...scoped,
          ...(overview
            ? {
                fleet: overview,
                selectedHostId: nextHostId,
              }
            : {}),
          hostDetails: nextDetails,
          gitReposCache: nextGitReposCache,
          ...(syncedSortMode ? { sidebarProjectSort: syncedSortMode } : {}),
          selectedAgentId: nextAgentId,
          isDraftNewSession: nextDraft,
          selectedWorkspaceCwd: nextCwd,
          draftProvider: nextDraftProvider,
          terminalTabs: nextTabs,
          activeTerminalTabKey: nextActiveTab,
          stages: remappedStages,
          composerDrafts:
            remotelyArchivedDraftKeys.size > 0
              ? omitRecordKeys(remappedComposerDrafts, remotelyArchivedDraftKeys)
              : remappedComposerDrafts,
          slashDirectives:
            remotelyArchivedDraftKeys.size > 0
              ? omitRecordKeys(remappedSlashDirectives, remotelyArchivedDraftKeys)
              : remappedSlashDirectives,
          inlineMentions:
            remotelyArchivedDraftKeys.size > 0
              ? omitRecordKeys(remappedInlineMentions, remotelyArchivedDraftKeys)
              : remappedInlineMentions,
          contextItems: remappedContextItems,
          pastedAttachments:
            remotelyArchivedDraftKeys.size > 0
              ? omitRecordKeys(remappedPastedAttachments, remotelyArchivedDraftKeys)
              : remappedPastedAttachments,
          diffQuotes:
            remotelyArchivedDraftKeys.size > 0
              ? omitRecordKeys(remappedDiffQuotes, remotelyArchivedDraftKeys)
              : remappedDiffQuotes,
          sideQuestionsByScope: scoped.sideQuestionsByScope,
          sideQuestionDrafts:
            remotelyArchivedDraftKeys.size > 0
              ? omitRecordKeys(scoped.sideQuestionDrafts, remotelyArchivedDraftKeys)
              : scoped.sideQuestionDrafts,
        };
      });

      if (newlyAttachedTerminal !== null) {
        const attached = newlyAttachedTerminal as { tabKey: string; terminalId: string };
        ctx.actions.onTerminalTabAttached(hostId, attached.tabKey, attached.terminalId);
      }

      if (viewedAgentIdToMark !== null && viewedAgentLatestSeq !== null) {
        markThreadSeenInCore(
          hostId,
          viewedAgentIdToMark,
          viewedAgentLatestSeq,
          viewedAgentWasUnread,
        );
      }

      syncAttentionIndicators(getState().fleet, getState().hostDetails);
      ctx.actions.refreshSidebarView();
      maybeAutoSyncOutdatedHosts();
      void drainPromptQueueIfIdle(hostId, detail);

      if (settledGitRefreshCwd) {
        scheduleAutoRefreshGit(hostId, settledGitRefreshCwd, {
          delayMs: 800,
          reason: "settled",
        });
      }

      if (newestCreated) {
        void orbitIpc.openAgentTimeline(hostId, newestCreated.agent_id);
        void orbitIpc.listProviderSubagents(hostId, newestCreated.agent_id);
        void ctx.actions.pullStageDelta(hostId, newestCreated.agent_id, true);
      }
    },


    selectHost(hostId: string, opts?: { keepDraft?: boolean }): void {
      flushPendingSeenMarks();
      ctx.setPendingDraftCreate(null);
      const detail = getState().hostDetails[hostId];
      const stayInDraft = Boolean(opts?.keepDraft);
      const firstAgent = stayInDraft
        ? null
        : (detail?.agents.find(
            (a) =>
              !a.parent_agent_id &&
              !a.background &&
              !a.archived_at_rfc3339 &&
              !getState().archivedThreads[threadScopeKey(hostId, a.agent_id)],
          ) ?? null);
      const sidebarHostProj = getState().sidebarView?.projects.find(
        (p) => p.host_id === hostId && !p.is_home_workspace,
      );
      const firstValidWorkspace = detail?.workspaces.find(
        (w) => !w.archived_at && !w.background && !w.is_worktree && !isBareHomeDir(w.cwd),
      );
      const firstDiscovered = detail?.discovered_projects.find(
        (p) => !isBareHomeDir(p.path) && !p.path.includes("/worktrees/"),
      );
      const nextCwd =
        firstAgent?.cwd ??
        sidebarHostProj?.primary_cwd ??
        firstValidWorkspace?.project_root ??
        firstValidWorkspace?.cwd ??
        firstDiscovered?.path ??
        detail?.discovered_projects[0]?.path ??
        "~";
      const nextAgentId = firstAgent?.agent_id ?? null;
      const nextIsDraft = stayInDraft || firstAgent === null;
      setState((s) => {
        const nextScopeKey = resolveSidePanelScopeKey({
          selectedHostId: hostId,
          selectedAgentId: nextAgentId,
          isDraftNewSession: nextIsDraft,
          hostDetails: s.hostDetails,
        });
        const scoped = switchSidePanelScope(s, nextScopeKey);
        return {
          ...scoped,
          activeView: "workspace",
          workspacePanel: "conversation",
          selectedHostId: hostId,
          selectedAgentId: nextAgentId,
          isDraftNewSession: nextIsDraft,
          sessionScrollNonce: s.sessionScrollNonce + 1,
          pendingJumpTarget: null,
          selectedWorkspaceCwd: nextCwd,
          selectedDiffPath: null,
          draftWorktreeRepoPath: null,
          draftWorkspaceId: null,
          draftProvider: resolveAvailableDraftProvider(detail, s.draftProvider),
        };
      });

      ctx.actions.refreshSidebarView();
      void orbitIpc.refreshQuotas(hostId);
      if (firstAgent) {
        const firstKey = threadScopeKey(hostId, firstAgent.agent_id);
        if (getState().stages[firstKey]) {
          ctx.recordStageKeyAccess(firstKey);
        }
        markThreadSeenInCore(hostId, firstAgent.agent_id, firstAgent.latest_seq);
        void orbitIpc.openAgentTimeline(hostId, firstAgent.agent_id);
        void orbitIpc.listProviderSubagents(hostId, firstAgent.agent_id);
        void ctx.actions.pullStageDelta(hostId, firstAgent.agent_id, false);
      }
      if (nextCwd && nextCwd !== "~") {
        void ctx.actions.refreshGit(hostId, nextCwd, null, getState().diffStaged);
        void ctx.actions.inspectGitRepos(hostId, nextCwd);
      }
    },


    selectAgent(hostId: string, agentId: string, jumpToSeq?: number | null): void {
      flushPendingSeenMarks();
      ctx.setPendingDraftCreate(null);
      const detail = getState().hostDetails[hostId];
      const ag = detail?.agents.find((a) => a.agent_id === agentId);
      // Child subagents are inspected exclusively in the right Side Panel, never as a
      // standalone Main Stage view. Route any selection of a managed child to its root
      // parent thread and open the child inspector tab on the right.
      if (ag?.parent_agent_id) {
        let rootHostId = hostId;
        let rootAgentId = agentId;
        let cursor = ag;
        const visited = new Set<string>([`${hostId}::${agentId}`]);
        while (cursor?.parent_agent_id) {
          const nextHostId = cursor.parent_host_id ?? rootHostId;
          const nextAgentId = cursor.parent_agent_id;
          const visitKey = `${nextHostId}::${nextAgentId}`;
          if (visited.has(visitKey)) break;
          visited.add(visitKey);
          const parentAg = getState().hostDetails[nextHostId]?.agents.find(
            (a) => a.agent_id === nextAgentId,
          );
          if (!parentAg || parentAg.archived_at_rfc3339 || getState().archivedThreads[visitKey]) break;
          rootHostId = nextHostId;
          rootAgentId = nextAgentId;
          cursor = parentAg;
        }
        if (rootAgentId !== agentId || rootHostId !== hostId) {
          const snap = getState();
          const parentAlreadyActive =
            snap.activeView === "workspace" &&
            snap.workspacePanel === "conversation" &&
            !snap.isDraftNewSession &&
            snap.selectedHostId === rootHostId &&
            snap.selectedAgentId === rootAgentId;
          if (!parentAlreadyActive) {
            ctx.actions.selectAgent(rootHostId, rootAgentId);
          }
          ctx.actions.openSubagentTab({
            callId: ag.agent_id,
            role: ag.title || ag.agent_id,
            promptPreview: ag.task_summary ?? "",
            childAgentId: ag.agent_id,
            childHostId: hostId,
            parentHostId: rootHostId,
            parentAgentId: rootAgentId,
          });
          return;
        }
      }
      const isSwitchingThread =
        getState().activeView !== "workspace" ||
        getState().workspacePanel !== "conversation" ||
        getState().isDraftNewSession ||
        getState().selectedHostId !== hostId ||
        getState().selectedAgentId !== agentId;
      const nextCwd = ag?.cwd ?? getState().selectedWorkspaceCwd;
      const tKey = threadScopeKey(hostId, agentId);
      if (getState().stages[tKey]) {
        ctx.recordStageKeyAccess(tKey);
      }
      const seenSeq = Math.max(ag?.latest_seq ?? 0, getState().stages[tKey]?.latest_seq ?? 0);

      setState(
        (s) => {
          let nextArchived = s.archivedThreads;
          if (nextArchived[tKey]) {
            nextArchived = { ...nextArchived };
            delete nextArchived[tKey];
          }
          const nextScopeKey = resolveSidePanelScopeKey({
            selectedHostId: hostId,
            selectedAgentId: agentId,
            isDraftNewSession: false,
            hostDetails: s.hostDetails,
          });
          const scoped = switchSidePanelScope(s, nextScopeKey);
          const nextNonce = s.sessionScrollNonce + 1;
          const nextJumpTarget =
            typeof jumpToSeq === "number" && jumpToSeq > 0
              ? { hostId, agentId, seq: jumpToSeq, nonce: nextNonce }
              : null;
          return {
            ...scoped,
            activeView: "workspace",
            workspacePanel: "conversation",
            selectedHostId: hostId,
            selectedAgentId: agentId,
            isDraftNewSession: false,
            sessionScrollNonce: nextNonce,
            pendingJumpTarget: nextJumpTarget,
            selectedWorkspaceCwd: nextCwd,
            selectedDiffPath: null,
            archivedThreads: nextArchived,
          };
        },
        true,
      );

      markThreadSeenInCore(hostId, agentId, seenSeq, true);
      ctx.actions.refreshSidebarView();
      if (isSwitchingThread) {
        ctx.actions.focusComposer({ onlyIfIdle: true });
      }
      void orbitIpc.openAgentTimeline(hostId, agentId);
      void orbitIpc.markAgentRead(hostId, agentId);
      void orbitIpc.listProviderSubagents(hostId, agentId);
      void ctx.actions.pullStageDelta(hostId, agentId, false);
      if (ag?.workspace_id) {
        const ws = detail?.workspaces.find((w) => w.workspace_id === ag.workspace_id);
        const peerCount =
          detail?.agents.filter(
            (a) => a.workspace_id === ag.workspace_id && !a.archived_at_rfc3339,
          ).length ?? 0;
        if (peerCount >= 2 || (ws?.board_revision ?? 0) > 0) {
          void ctx.actions.fetchWorkspaceBoardAndEvents(hostId, ag.workspace_id);
        }
      }
      if (nextCwd && nextCwd !== "~") {
        void ctx.actions.refreshGit(hostId, nextCwd, null, getState().diffStaged);
      }
    },


    clearPendingJumpTarget(nonce: number): void {
      setState((s) =>
        s.pendingJumpTarget && s.pendingJumpTarget.nonce === nonce
          ? { ...s, pendingJumpTarget: null }
          : s,
      );
    },


    startNewThreadDraft(
      hostId?: string,
      cwd?: string,
      prefillPrompt?: string,
      prefillSlash?: ComposerInlineDirective | null,
      workspaceId?: string | null,
    ): void {
      ctx.setPendingDraftCreate(null);
      const targetHostId = hostId ?? getState().selectedHostId ?? getState().fleet.hosts[0]?.host_id ?? null;
      const detail = targetHostId ? getState().hostDetails[targetHostId] : undefined;
      const defaultCwd =
        cwd ||
        (targetHostId === getState().selectedHostId ? getState().selectedWorkspaceCwd : "") ||
        detail?.workspaces[0]?.cwd ||
        detail?.discovered_projects[0]?.path ||
        "~";

      const nextProvider = resolveAvailableDraftProvider(detail, getState().draftProvider);
      const draftKey = `draft::${targetHostId ?? "none"}`;

      setState((s) => {
        const nextScopeKey = targetHostId ? `draft::${targetHostId}` : null;
        const nextDraftCwd = defaultCwd || "~";
        const scoped = rebindDraftSidePanelCwd(
          { ...switchSidePanelScope(s, nextScopeKey), isDraftNewSession: true },
          nextDraftCwd,
        );
        return {
          ...scoped,
          activeView: "workspace",
          workspacePanel: "conversation",
          selectedHostId: targetHostId,
          selectedAgentId: null,
          isDraftNewSession: true,
          pendingJumpTarget: null,
          selectedWorkspaceCwd: nextDraftCwd,
          selectedDiffPath: null,
          draftWorktreeRepoPath: null,
          draftWorkspaceId: workspaceId ?? null,
          draftProvider: nextProvider,
          commandPaletteOpen: false,
          ...(prefillPrompt !== undefined
            ? {
                composerDrafts: {
                  ...s.composerDrafts,
                  [draftKey]: prefillPrompt,
                },
              }
            : {}),
          ...(prefillSlash !== undefined
            ? {
                slashDirectives: {
                  ...s.slashDirectives,
                  [draftKey]: prefillSlash,
                },
              }
            : {}),
        };
      });

      ctx.actions.refreshSidebarView();
      ctx.actions.focusComposer();
      if (targetHostId && defaultCwd && defaultCwd !== "~") {
        void ctx.actions.inspectGitRepos(targetHostId, defaultCwd);
      }
    },


    openAutomations(): void {
      setState((s) => ({
        ...s,
        activeView: "workspace",
        workspacePanel: "automations",
        commandPaletteOpen: false,
      }));
    },


    setPrimaryRouterHostId(hostId: string | null): void {
      setState((s) => (s.primaryRouterHostId === hostId ? s : { ...s, primaryRouterHostId: hostId }));
    },


    async refreshPrimaryRouterSelection(): Promise<string | null> {
      const sel = await orbitIpc.getPrimaryRouterSelection().catch(() => null);
      const nextHostId = sel?.host_id ?? null;
      setState((s) =>
        s.primaryRouterHostId === nextHostId ? s : { ...s, primaryRouterHostId: nextHostId },
      );
      return nextHostId;
    },


    openGlobalOverview(): void {
      const snap = getState();
      const personalAgent = selectPersonalAgentRef(snap);

      // Refresh pending permissions across connected hosts for fresh triage accuracy
      for (const h of snap.fleet.hosts) {
        if (h.connection_state === "online_direct" || h.connection_state === "online_relay") {
          void orbitIpc.refreshPendingPermissions(h.host_id).catch(() => undefined);
        }
      }

      if (personalAgent) {
        setState((s) =>
          s.commandPaletteOpen || s.overviewPopoverOpen
            ? { ...s, commandPaletteOpen: false, overviewPopoverOpen: false }
            : s,
        );
        ctx.actions.selectAgent(personalAgent.hostId, personalAgent.agentId);
        return;
      }

      setState((s) => ({
        ...s,
        activeView: "workspace",
        workspacePanel: "overview",
        commandPaletteOpen: false,
        overviewPopoverOpen: false,
      }));
    },


    focusComposer(opts?: ComposerFocusOptions): void {
      setTimeout(() => {
        for (const fn of composerFocusListeners) {
          fn(opts);
        }
      }, 16);
    },


    onComposerFocusRequest(cb: (opts?: ComposerFocusOptions) => void): () => void {
      composerFocusListeners.add(cb);
      return () => composerFocusListeners.delete(cb);
    },


    setSelectedWorkspaceCwd(cwd: string): void {
      setState((s) => ({
        ...rebindDraftSidePanelCwd(s, cwd),
        selectedWorkspaceCwd: cwd,
        selectedDiffPath: null,
        draftWorktreeRepoPath: null,
        draftWorkspaceId: null,
      }));
      const snap = getState();
      if (snap.selectedHostId && cwd && cwd !== "~") {
        void ctx.actions.refreshGit(snap.selectedHostId, cwd, null, snap.diffStaged);
        void ctx.actions.inspectGitRepos(snap.selectedHostId, cwd);
      }
    },


    selectProjectWorkspace(hostId: string, cwd: string): void {
      const detail = getState().hostDetails[hostId];
      setState((s) => {
        const nextAgentId = s.isDraftNewSession ? null : s.selectedAgentId;
        const nextScopeKey = resolveSidePanelScopeKey({
          selectedHostId: hostId,
          selectedAgentId: nextAgentId,
          isDraftNewSession: s.isDraftNewSession,
          hostDetails: s.hostDetails,
        });
        const scoped = rebindDraftSidePanelCwd(switchSidePanelScope(s, nextScopeKey), cwd);
        return {
          ...scoped,
          selectedHostId: hostId,
          selectedAgentId: nextAgentId,
          selectedWorkspaceCwd: cwd,
          selectedDiffPath: null,
          draftWorktreeRepoPath: null,
          draftWorkspaceId: null,
          draftProvider: resolveAvailableDraftProvider(detail, s.draftProvider),
        };
      });
      ctx.actions.refreshSidebarView();
      void ctx.actions.ensureWorkspaceFiles(hostId, cwd);
      if (cwd && cwd !== "~") {
        void ctx.actions.refreshGit(hostId, cwd, null, getState().diffStaged);
        void ctx.actions.inspectGitRepos(hostId, cwd);
      }
    },


    jumpToNextAttentionItem(): void {
      for (const host of getState().fleet.hosts) {
        const detail = getState().hostDetails[host.host_id];
        if (!detail) continue;
        const blocked = detail.agents.find(
          (a) =>
            a.is_awaiting_permission &&
            !getState().archivedThreads[threadScopeKey(host.host_id, a.agent_id)],
        );
        if (blocked) {
          ctx.actions.selectAgent(host.host_id, blocked.agent_id);
          return;
        }
      }
      notifyToast("All agents clear — no pending approvals");
    },


    selectThreadByIndex(indexZeroBased: number): void {
      const snap = getState();
      const ordered = collectVisibleSidebarThreads({
        hosts: snap.fleet.hosts,
        sidebarView: snap.sidebarView,
        hostDetails: snap.hostDetails,
        archivedThreads: snap.archivedThreads,
        hiddenProjectKeys: snap.hiddenProjectKeys,
        threadCustomTitles: snap.threadCustomTitles,
        collapsedProjectIds: snap.collapsedProjectIds,
        collapsedWorkspaceIds: snap.collapsedWorkspaceIds,
        sidebarProjectSort: snap.sidebarProjectSort,
        threadManualOrder: snap.threadManualOrder,
        workspacePanel: snap.workspacePanel,
        isDraftNewSession: snap.isDraftNewSession,
        selectedHostId: snap.selectedHostId,
        selectedAgentId: snap.selectedAgentId,
        selectedWorkspaceCwd: snap.selectedWorkspaceCwd,
      });
      const target = ordered[indexZeroBased];
      if (target) {
        ctx.actions.selectAgent(target.hostId, target.agentId);
      }
    },


    togglePinThread(hostId: string, agentId: string): void {
      void orbitIpc.togglePinAgent(hostId, agentId).then(() => {
        ctx.actions.refreshSidebarView();
      });
    },


    renameThread(hostId: string, agentId: string, newTitle: string): void {
      const tKey = threadScopeKey(hostId, agentId);
      const trimmed = newTitle.trim();
      const originalTitle =
        getState().hostDetails[hostId]?.agents.find((a) => a.agent_id === agentId)?.title ?? "";
      setState(
        (s) => {
          const nextTitles = { ...s.threadCustomTitles };
          if (!trimmed || trimmed === originalTitle) {
            delete nextTitles[tKey];
          } else {
            nextTitles[tKey] = trimmed;
          }
          return { ...s, threadCustomTitles: nextTitles };
        },
        true,
      );
      if (trimmed && trimmed !== originalTitle) {
        void orbitIpc.updateAgent({ hostId, agentId, title: trimmed });
      }
    },


    markThreadUnread(hostId: string, agentId: string): void {
      const tKey = threadScopeKey(hostId, agentId);
      const isCurrentlySelected =
        !getState().isDraftNewSession &&
        getState().selectedHostId === hostId &&
        getState().selectedAgentId === agentId;

      lastReportedSeenSeq.delete(tKey);
      if (isCurrentlySelected) {
        setState(
          (s) => {
            const scoped = switchSidePanelScope(s, `draft::${hostId}`);
            return {
              ...scoped,
              isDraftNewSession: true,
              selectedAgentId: null,
              selectedDiffPath: null,
            };
          },
          true,
        );
      }
      void orbitIpc.markAgentUnread(hostId, agentId).then(() => {
        ctx.actions.refreshSidebarView();
      });
    },


    markThreadRead(hostId: string, agentId: string): void {
      const tKey = threadScopeKey(hostId, agentId);
      const detail = getState().hostDetails[hostId];
      const ag =
        detail?.agents.find((a) => a.agent_id === agentId) ??
        getState().stages[tKey]?.agent ??
        null;
      const targetSeq = Math.max(
        ag?.latest_seq ?? 0,
        getState().stages[tKey]?.latest_seq ?? 0,
        1,
      );
      lastReportedSeenSeq.set(tKey, targetSeq);
      void Promise.all([
        orbitIpc.markAgentSeen(hostId, agentId, targetSeq),
        orbitIpc.markAgentRead(hostId, agentId),
      ]).then(() => {
        ctx.actions.refreshSidebarView();
      });
    },


    openExportDialog(hostId: string, agentId: string): void {
      setState((s) => ({
        ...s,
        exportDialogTarget: { hostId, agentId },
      }));
    },


    closeExportDialog(): void {
      setState((s) => (s.exportDialogTarget ? { ...s, exportDialogTarget: null } : s));
    },


    openWorktreeHandoffDialog(target: WorktreeHandoffDialogTarget): void {
      setState((s) => ({
        ...s,
        handoffDialogTarget: target,
      }));
    },


    closeWorktreeHandoffDialog(): void {
      setState((s) => (s.handoffDialogTarget ? { ...s, handoffDialogTarget: null } : s));
    },


    async openCopyCodeBlocksModal(hostId: string, agentId: string): Promise<void> {
      const reqSeq = ++copyBlocksReqSeq;
      let blocks: ExtractedCodeBlock[] = [];
      try {
        blocks = await orbitIpc.listAgentCodeBlocks(hostId, agentId);
      } catch {
        blocks = [];
      }
      if (
        reqSeq !== copyBlocksReqSeq ||
        getState().selectedHostId !== hostId ||
        getState().selectedAgentId !== agentId
      ) {
        return;
      }

      if (blocks.length > 0) {
        setState((s) => ({
          ...s,
          copyCodeBlocksTarget: { hostId, agentId, blocks },
        }));
        return;
      }

      // Fallback when 0 code blocks exist: copy the latest assistant reply directly.
      const tKey = threadScopeKey(hostId, agentId);
      let lastReply = "";
      const cachedStage = getState().stages[tKey];
      if (cachedStage && cachedStage.turns.length > 0) {
        for (let t = cachedStage.turns.length - 1; t >= 0 && !lastReply; t -= 1) {
          const segs = cachedStage.turns[t]?.live_segments ?? [];
          for (let sIdx = segs.length - 1; sIdx >= 0; sIdx -= 1) {
            const seg = segs[sIdx];
            if (seg?.kind === "assistant_message" && seg.item.item_type === "assistant_message") {
              const trimmed = seg.item.text.trim();
              if (trimmed) {
                lastReply = trimmed;
                break;
              }
            }
          }
        }
      }
      if (!lastReply) {
        try {
          const full = await orbitIpc.getAgentTimeline(hostId, agentId);
          if (
            reqSeq !== copyBlocksReqSeq ||
            getState().selectedHostId !== hostId ||
            getState().selectedAgentId !== agentId
          ) {
            return;
          }
          for (let i = full.items.length - 1; i >= 0; i -= 1) {
            const item = full.items[i];
            if (item?.item_type === "assistant_message") {
              const trimmed = item.text.trim();
              if (trimmed) {
                lastReply = trimmed;
                break;
              }
            }
          }
        } catch {
          // ignore fallback fetch errors
        }
      }

      if (
        reqSeq !== copyBlocksReqSeq ||
        getState().selectedHostId !== hostId ||
        getState().selectedAgentId !== agentId
      ) {
        return;
      }
      if (!lastReply) {
        notifyToast("No assistant reply or code block to copy");
        return;
      }
      try {
        await navigator.clipboard.writeText(lastReply);
        notifyToast("Copied last assistant reply");
      } catch {
        notifyToast("Failed to copy reply");
      }
    },


    closeCopyCodeBlocksModal(): void {
      copyBlocksReqSeq += 1;
      setState((s) => (s.copyCodeBlocksTarget ? { ...s, copyCodeBlocksTarget: null } : s));
    },


    async copyThread(hostId: string, agentId: string): Promise<void> {
      try {
        const exported = await orbitIpc.exportAgentTranscript(hostId, agentId, {
          format: "markdown",
          include_thinking: false,
          include_tool_calls: false,
          include_timestamps: false,
        });
        await navigator.clipboard.writeText(exported.content);
        notifyToast("Copied conversation");
        return;
      } catch {
        // Fallback for non-Tauri browser preview
      }

      const tKey = threadScopeKey(hostId, agentId);
      const sections: string[] = [];
      const cachedStage = getState().stages[tKey];
      if (cachedStage && cachedStage.turns.length > 0) {
        for (const turn of cachedStage.turns) {
          if (turn.user_prompt?.item_type === "user_prompt") {
            const txt = turn.user_prompt.text.trim();
            if (txt) sections.push(`## User\n\n${txt}`);
          }
          for (const seg of turn.live_segments) {
            if (seg.kind === "assistant_message" && seg.item.item_type === "assistant_message") {
              const txt = seg.item.text.trim();
              if (txt) sections.push(`## Assistant\n\n${txt}`);
            }
          }
        }
      } else {
        let items: Awaited<ReturnType<typeof orbitIpc.getAgentTimeline>>["items"];
        try {
          const full = await orbitIpc.getAgentTimeline(hostId, agentId);
          items = full.items;
        } catch {
          items = [];
        }
        for (const item of items) {
          if (item.item_type === "user_prompt") {
            const txt = item.text.trim();
            if (txt) sections.push(`## User\n\n${txt}`);
          } else if (item.item_type === "assistant_message") {
            const txt = item.text.trim();
            if (txt) sections.push(`## Assistant\n\n${txt}`);
          }
        }
      }

      const fallbackTitle =
        getState().threadCustomTitles[tKey] ||
        getState().hostDetails[hostId]?.agents.find((a) => a.agent_id === agentId)?.title ||
        agentId;
      const payload = sections.length > 0 ? sections.join("\n\n---\n\n") : fallbackTitle;
      try {
        await navigator.clipboard.writeText(payload);
        notifyToast("Copied conversation");
      } catch {
        notifyToast("Failed to copy conversation");
      }
    },


    async rewindAgent(
      hostId: string,
      agentId: string,
      toBeforeSeq: number,
      restoreFiles: boolean,
      fallbackPromptText?: string,
      fallbackBadges?: readonly PromptBadge[],
      restorePaths?: string[] | null,
    ): Promise<boolean> {
      const tKey = threadScopeKey(hostId, agentId);
      // Clear queued follow-ups before calling the daemon so transitioning to Idle never auto-dispatches stale prompts
      setState((s) => {
        if (!s.queuedPrompts[tKey]?.length) return s;
        const nextQueued = { ...s.queuedPrompts };
        delete nextQueued[tKey];
        return { ...s, queuedPrompts: nextQueued };
      });

      try {
        const res = await orbitIpc.rewindAgent(
          hostId,
          agentId,
          toBeforeSeq,
          restoreFiles,
          restorePaths ?? null,
        );
        const rawFallback = (fallbackPromptText ?? "").trim();
        const rawServerPrompt = (res.restored_prompt ?? "").trim();
        const hasFallbackBadges = Boolean(fallbackBadges && fallbackBadges.length > 0);

        // Reconstruct composer attachments (images, files, media, diffs) & slash directive from the rewound prompt
        const restoredContexts: ContextMentionItem[] = [];
        let restoredSlash: SlashDirective | null = null;
        const nowToken = Date.now();
        let imageIdx = 0;

        if (fallbackBadges && fallbackBadges.length > 0) {
          for (let i = 0; i < fallbackBadges.length; i += 1) {
            const badge = fallbackBadges[i];
            if (badge.kind === "image") {
              const recent = lookupRecentSentTurnImage(
                hostId,
                agentId,
                rawFallback || rawServerPrompt,
                imageIdx,
                badge.label,
              );
              imageIdx += 1;
              const resolvedPath = badge.path?.trim() || recent?.path?.trim() || "";
              if (!resolvedPath) continue;
              const cached = getCachedPromptImage(hostId, resolvedPath);
              restoredContexts.push({
                id: `img_rw_${nowToken}_${i}`,
                kind: "image",
                filename: badge.label || basenameFromPath(resolvedPath) || "image.png",
                path: resolvedPath,
                previewUrl:
                  (cached?.status === "ready" ? cached.url : null) ?? recent?.previewUrl ?? "",
                sizeBytes: 0,
                stagedHostId: hostId,
              });
            } else if (
              badge.kind === "pdf" ||
              badge.kind === "audio" ||
              badge.kind === "video"
            ) {
              const resolvedPath = badge.path?.trim() || "";
              if (!resolvedPath) continue;
              restoredContexts.push({
                id: `${badge.kind}_rw_${nowToken}_${i}`,
                kind: "file",
                path: resolvedPath,
                filename: badge.label || basenameFromPath(resolvedPath),
                stagedHostId: hostId,
                mediaKind: badge.kind,
                mimeType: defaultMimeForMedia(badge.label || resolvedPath, badge.kind),
              });
            } else if (badge.kind === "file") {
              const resolvedPath = badge.path?.trim() || "";
              if (!resolvedPath) continue;
              const isStaged = resolvedPath.startsWith("/");
              restoredContexts.push({
                id: `file_rw_${nowToken}_${i}`,
                kind: "file",
                path: resolvedPath,
                filename: badge.label || basenameFromPath(resolvedPath),
                ...(isStaged ? { stagedHostId: hostId } : {}),
              });
            } else if (badge.kind === "git_diff") {
              const staged = /staged/i.test(badge.label);
              restoredContexts.push({
                id: `diff_rw_${staged ? "s" : "w"}_${nowToken}_${i}`,
                kind: "git_diff",
                staged,
                label: badge.label || (staged ? "Staged Diff" : "Working Tree Diff"),
              });
            } else if (badge.kind === "slash") {
              const rawCmd = badge.label.replace(/^\//, "").trim();
              if (
                rawCmd === "plan" ||
                rawCmd === "review" ||
                rawCmd === "compact" ||
                rawCmd === "orbit" ||
                rawCmd === "orbit-handoff" ||
                rawCmd === "orbit-committee" ||
                rawCmd === "orbit-advisor"
              ) {
                restoredSlash = rawCmd;
              }
            }
          }
        }

        // If the prompt came from a legacy wire format (e.g. `Attached image file: \`/path\``) and we
        // already extracted structured badges, prefer the clean text so raw attachment markup isn't duplicated.
        const serverLooksLikeLegacyMarkup =
          hasFallbackBadges &&
          /^Attached (?:image|PDF|audio|video)? ?file:\s*`[^`]+`/m.test(rawServerPrompt);
        const restoredDraft = serverLooksLikeLegacyMarkup
          ? rawFallback
          : rawServerPrompt || rawFallback;
        const cwd = res.agent?.cwd || getState().selectedWorkspaceCwd;

        const openFileTabsToReload: Array<{ hostId: string; cwd: string; path: string }> = [];
        setState((s) => {
          let nextFileCache = s.fileViewCache;
          if (restoreFiles && res.restored_files > 0 && cwd && cwd !== "~") {
            const prefix = `${hostId}::${cwd}::`;
            nextFileCache = {};
            for (const [k, v] of Object.entries(s.fileViewCache)) {
              if (!k.startsWith(prefix)) {
                nextFileCache[k] = v;
              } else {
                bumpFileLoadSeq(k);
              }
            }
            for (const tab of s.sidePanelTabs) {
              if (tab.kind === "file" && tab.filePath && tab.hostId === hostId && tab.cwd === cwd) {
                openFileTabsToReload.push({ hostId: tab.hostId, cwd: tab.cwd, path: tab.filePath });
              }
            }
          }

          // Preserve any images/files already staged in the composer AND merge restored attachments from the rewound turn
          const existingCtxs = s.contextItems[tKey] ?? [];
          const mergedCtxs = [...existingCtxs];
          for (const item of restoredContexts) {
            const dup = mergedCtxs.some((cur) => {
              if (cur.kind === "image" && item.kind === "image") {
                return Boolean(cur.path && cur.path === item.path);
              }
              if (cur.kind === "file" && item.kind === "file") {
                return Boolean(cur.path && cur.path === item.path);
              }
              if (cur.kind === "git_diff" && item.kind === "git_diff") {
                return cur.staged === item.staged;
              }
              return false;
            });
            if (!dup) mergedCtxs.push(item);
          }

          return {
            ...s,
            fileViewCache: nextFileCache,
            rewoundSessions: { ...s.rewoundSessions, [tKey]: true },
            composerDrafts: restoredDraft
              ? { ...s.composerDrafts, [tKey]: restoredDraft }
              : s.composerDrafts,
            contextItems:
              mergedCtxs.length !== existingCtxs.length
                ? { ...s.contextItems, [tKey]: mergedCtxs }
                : s.contextItems,
            slashDirectives:
              restoredSlash && !s.slashDirectives[tKey]
                ? { ...s.slashDirectives, [tKey]: restoredSlash }
                : s.slashDirectives,
          };
        });
        draftPersistence.flushNow();

        await Promise.all([
          ctx.actions.pullStageDelta(hostId, agentId, true),
          ctx.actions.reloadHostDetail(hostId, true),
        ]);

        for (const tabInfo of openFileTabsToReload) {
          void ctx.actions.loadWorkspaceFile(tabInfo.hostId, tabInfo.cwd, tabInfo.path, true);
        }

        if (cwd && cwd !== "~") {
          void ctx.actions.refreshGit(hostId, cwd, null, getState().diffStaged);
        }

        if (res.notice) {
          notifyToast(res.notice);
        } else if (restoreFiles && res.restored_files > 0) {
          notifyToast(
            `Rewound conversation · Restored ${res.restored_files} ${res.restored_files === 1 ? "file" : "files"}`,
          );
        } else {
          notifyToast("Rewound conversation to this point");
        }

        setTimeout(() => {
          for (const fn of composerFocusListeners) {
            fn();
          }
        }, 16);
        return true;
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        notifyToast(msg || "Failed to rewind conversation");
        return false;
      }
    },


    async forkAgent(opts: {
      hostId: string;
      sourceAgentId: string;
      upToSeq?: number | null;
      worktree?: boolean;
      branch?: string;
      baseRef?: string;
      title?: string;
      initialPrompt?: string;
    }): Promise<boolean> {
      const dedupKey = `${opts.hostId}::${opts.sourceAgentId}`;
      if (inFlightForks.has(dedupKey)) {
        return false;
      }
      inFlightForks.add(dedupKey);
      try {
        const useWorktree = Boolean(
          opts.worktree || opts.branch?.trim() || opts.baseRef?.trim(),
        );
        const res = await orbitIpc.forkAgent({
          host_id: opts.hostId,
          source_agent_id: opts.sourceAgentId,
          up_to_seq: opts.upToSeq ?? null,
          target: useWorktree
            ? {
                kind: "new_worktree",
                branch: opts.branch?.trim() || null,
                base_ref: opts.baseRef?.trim() || null,
              }
            : { kind: "same_directory" },
          title: opts.title?.trim() || null,
          initial_prompt: opts.initialPrompt?.trim() || null,
        });

        await ctx.actions.reloadHostDetail(opts.hostId, true);
        ctx.actions.selectAgent(opts.hostId, res.agent.agent_id);

        if (res.workspace?.is_worktree) {
          const branchNote = res.workspace.git_branch
            ? ` · ${res.workspace.git_branch}`
            : "";
          notifyToast(`Forked in new worktree${branchNote}`);
        }

        setTimeout(() => {
          for (const fn of composerFocusListeners) {
            fn();
          }
        }, 24);
        return true;
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        notifyToast(msg || "Failed to fork conversation");
        return false;
      } finally {
        inFlightForks.delete(dedupKey);
      }
    },


    archiveThread(hostId: string, agentId: string): void {
      const tKey = threadScopeKey(hostId, agentId);
      const wasSelected =
        !getState().isDraftNewSession &&
        getState().selectedHostId === hostId &&
        getState().selectedAgentId === agentId;

      const detail = getState().hostDetails[hostId];
      const targetAgent = detail?.agents.find((a) => a.agent_id === agentId);
      let fallbackAgentId: string | null = null;
      if (wasSelected) {
        const nextCandidate = detail?.agents.find(
          (a) =>
            a.agent_id !== agentId &&
            !a.parent_agent_id &&
            !a.background &&
            !a.archived_at_rfc3339 &&
            !getState().archivedThreads[threadScopeKey(hostId, a.agent_id)],
        );
        fallbackAgentId = nextCandidate?.agent_id ?? null;
      }

      const preArchive = getState();
      const undoEntry: UndoArchiveEntry = {
        hostId,
        agentId,
        title: targetAgent?.title ?? agentId,
        draft: preArchive.composerDrafts[tKey] ?? "",
        slashDirective: preArchive.slashDirectives[tKey] ?? null,
        inlineMentions: [...(preArchive.inlineMentions[tKey] ?? [])],
        contexts: [...(preArchive.contextItems[tKey] ?? [])],
        pastes: [...(preArchive.pastedAttachments[tKey] ?? [])],
        quotes: [...(preArchive.diffQuotes[tKey] ?? [])],
        sideQuestionDraft: preArchive.sideQuestionDrafts[tKey] ?? "",
      };

      const archivedKeySet = new Set([tKey]);
      const removedSidePanelAgents: Array<{ hostId: string; agentId: string }> = [
        { hostId, agentId },
      ];
      const removedSidePanelKeys = new Set([tKey]);
      for (const [hId, hDetail] of Object.entries(preArchive.hostDetails)) {
        for (const ag of hDetail.agents) {
          if (ag.parent_agent_id === agentId && (ag.parent_host_id ?? hId) === hostId) {
            removedSidePanelAgents.push({ hostId: hId, agentId: ag.agent_id });
            removedSidePanelKeys.add(threadScopeKey(hId, ag.agent_id));
          }
        }
      }
      setState(
        (s) => {
          let scoped = dropSidePanelScopes(s, removedSidePanelKeys, removedSidePanelAgents);
          if (wasSelected && !fallbackAgentId) {
            scoped = switchSidePanelScope(scoped, `draft::${hostId}`, { dropCurrent: true });
          }
          return {
            ...scoped,
            archivedThreads: {
              ...s.archivedThreads,
              [tKey]: {
                key: tKey,
                hostId,
                agentId,
                archivedAtMs: Date.now(),
              },
            },
            composerDrafts: omitRecordKeys(s.composerDrafts, archivedKeySet),
            slashDirectives: omitRecordKeys(s.slashDirectives, archivedKeySet),
            inlineMentions: omitRecordKeys(s.inlineMentions, archivedKeySet),
            contextItems: omitRecordKeys(s.contextItems, archivedKeySet),
            pastedAttachments: omitRecordKeys(s.pastedAttachments, archivedKeySet),
            diffQuotes: omitRecordKeys(s.diffQuotes, archivedKeySet),
            sideQuestionDrafts: omitRecordKeys(s.sideQuestionDrafts, archivedKeySet),
            ...(wasSelected && !fallbackAgentId
              ? {
                  selectedAgentId: null,
                  isDraftNewSession: true,
                }
              : {}),
          };
        },
        true,
      );
      draftPersistence.removeScopes(archivedKeySet, true);

      void orbitIpc.archiveAgent(hostId, agentId);
      ctx.actions.refreshSidebarView();
      if (wasSelected && fallbackAgentId) {
        ctx.actions.selectAgent(hostId, fallbackAgentId);
      }
      notifyToast(t("store.conversationArchived"), undoEntry);
    },


    restoreArchivedThread(
      hostId: string,
      agentId: string,
      options?: { open?: boolean },
    ): void {
      const shouldOpen = options?.open !== false;
      const tKey = threadScopeKey(hostId, agentId);
      const targetAgent = getState().hostDetails[hostId]?.agents.find((a) => a.agent_id === agentId);
      const keysToUnhide = new Set<string>();
      if (targetAgent?.cwd) {
        keysToUnhide.add(projectPathScopeKey(hostId, targetAgent.cwd));
        for (const proj of getState().sidebarView?.projects ?? []) {
          if (proj.host_id === hostId && matchesProjectDirectory(proj.directories, targetAgent.cwd)) {
            keysToUnhide.add(projectPathScopeKey(hostId, proj.primary_cwd));
            for (const dir of proj.directories) {
              keysToUnhide.add(projectPathScopeKey(hostId, dir));
            }
          }
        }
      }

      setState(
        (s) => {
          const nextArchived = { ...s.archivedThreads };
          delete nextArchived[tKey];
          return {
            ...s,
            archivedThreads: nextArchived,
            hiddenProjectKeys:
              keysToUnhide.size > 0
                ? omitRecordKeys(s.hiddenProjectKeys, keysToUnhide)
                : s.hiddenProjectKeys,
            lastArchivedUndo: null,
            ...(shouldOpen ? { activeView: "workspace" as const } : {}),
          };
        },
        true,
      );
      void orbitIpc.unarchiveAgent(hostId, agentId);
      ctx.actions.refreshSidebarView();
      if (shouldOpen) {
        ctx.actions.selectAgent(hostId, agentId);
      }
      notifyToast(t("store.conversationRestored"));
    },


    async deleteArchivedThreadsBatch(
      items: { hostId: string; agentId: string }[],
    ): Promise<void> {
      if (items.length === 0) return;
      const deletedThreadKeys = new Set(items.map((it) => threadScopeKey(it.hostId, it.agentId)));

      setState(
        (s) => {
          for (const tKey of deletedThreadKeys) {
            for (const ctx of s.contextItems[tKey] ?? []) {
              if (
                (ctx.kind === "image" || ctx.kind === "file") &&
                ctx.previewUrl?.startsWith("blob:")
              ) {
                URL.revokeObjectURL(ctx.previewUrl);
              }
            }
          }
          const scoped = dropSidePanelScopes(s, deletedThreadKeys, items);
          return {
            ...scoped,
            archivedThreads: omitRecordKeys(s.archivedThreads, deletedThreadKeys),
            threadCustomTitles: omitRecordKeys(s.threadCustomTitles, deletedThreadKeys),
            stages: omitRecordKeys(s.stages, deletedThreadKeys),
            stageRunningSubagentMeta: omitRecordKeys(
              s.stageRunningSubagentMeta,
              deletedThreadKeys,
            ),
            composerDrafts: omitRecordKeys(s.composerDrafts, deletedThreadKeys),
            slashDirectives: omitRecordKeys(s.slashDirectives, deletedThreadKeys),
            inlineMentions: omitRecordKeys(s.inlineMentions, deletedThreadKeys),
            contextItems: omitRecordKeys(s.contextItems, deletedThreadKeys),
            pastedAttachments: omitRecordKeys(s.pastedAttachments, deletedThreadKeys),
            diffQuotes: omitRecordKeys(s.diffQuotes, deletedThreadKeys),
            queuedPrompts: omitRecordKeys(s.queuedPrompts, deletedThreadKeys),
            sideQuestionsByScope: omitRecordKeys(s.sideQuestionsByScope, deletedThreadKeys),
            sideQuestionDrafts: omitRecordKeys(s.sideQuestionDrafts, deletedThreadKeys),
            sessionCreatedPrs: omitRecordKeys(s.sessionCreatedPrs, deletedThreadKeys),
            rewoundSessions: omitRecordKeys(s.rewoundSessions, deletedThreadKeys),
          };
        },
        true,
      );
      draftPersistence.removeScopes(deletedThreadKeys, true);

      for (const tKey of deletedThreadKeys) {
        lastReportedSeenSeq.delete(tKey);
        stagePullGates.delete(tKey);
      }
      ctx.pruneStageKeyAccess(deletedThreadKeys);

      await Promise.allSettled(
        items.map((it) => orbitIpc.deleteAgent(it.hostId, it.agentId)),
      );
      const affectedHosts = new Set(items.map((it) => it.hostId));
      await Promise.allSettled(
        Array.from(affectedHosts).map((hid) => ctx.actions.reloadHostDetail(hid, false)),
      );
      ctx.actions.refreshSidebarView();
      notifyToast(
        items.length === 1
          ? "Deleted 1 archived conversation"
          : `Deleted ${items.length} archived conversations`,
      );
    },


    undoLastArchive(): void {
      const entry = getState().lastArchivedUndo;
      if (!entry) return;
      const tKey = threadScopeKey(entry.hostId, entry.agentId);
      const hasDraftSnapshot =
        Boolean(entry.draft) ||
        Boolean(entry.slashDirective) ||
        Boolean(entry.inlineMentions?.length) ||
        Boolean(entry.contexts?.length) ||
        Boolean(entry.pastes?.length) ||
        Boolean(entry.quotes?.length) ||
        Boolean(entry.sideQuestionDraft);

      ctx.actions.restoreArchivedThread(entry.hostId, entry.agentId);

      if (hasDraftSnapshot) {
        setState((s) => {
          const curDraft = s.composerDrafts[tKey] ?? "";
          const curDirective = s.slashDirectives[tKey] ?? null;
          const curInline = s.inlineMentions[tKey] ?? [];
          const curContexts = s.contextItems[tKey] ?? [];
          const curPastes = s.pastedAttachments[tKey] ?? [];
          const curQuotes = s.diffQuotes[tKey] ?? [];
          const curSideQuestion = s.sideQuestionDrafts[tKey] ?? "";

          return {
            ...s,
            composerDrafts:
              !curDraft && entry.draft
                ? { ...s.composerDrafts, [tKey]: entry.draft }
                : s.composerDrafts,
            slashDirectives:
              !curDirective && entry.slashDirective
                ? { ...s.slashDirectives, [tKey]: entry.slashDirective }
                : s.slashDirectives,
            inlineMentions:
              curInline.length === 0 && entry.inlineMentions && entry.inlineMentions.length > 0
                ? { ...s.inlineMentions, [tKey]: [...entry.inlineMentions] }
                : s.inlineMentions,
            contextItems:
              curContexts.length === 0 && entry.contexts && entry.contexts.length > 0
                ? { ...s.contextItems, [tKey]: [...entry.contexts] }
                : s.contextItems,
            pastedAttachments:
              curPastes.length === 0 && entry.pastes && entry.pastes.length > 0
                ? { ...s.pastedAttachments, [tKey]: [...entry.pastes] }
                : s.pastedAttachments,
            diffQuotes:
              curQuotes.length === 0 && entry.quotes && entry.quotes.length > 0
                ? { ...s.diffQuotes, [tKey]: [...entry.quotes] }
                : s.diffQuotes,
            sideQuestionDrafts:
              !curSideQuestion && entry.sideQuestionDraft
                ? { ...s.sideQuestionDrafts, [tKey]: entry.sideQuestionDraft }
                : s.sideQuestionDrafts,
          };
        });
        draftPersistence.flushNow();
      }
    },


    async detachAgent(hostId: string, agentId: string): Promise<void> {
      await orbitIpc.detachAgent(hostId, agentId);
      ctx.actions.refreshSidebarView();
      notifyToast("Detached subagent to standalone thread");
    },


    async deleteThreadPermanently(hostId: string, agentId: string): Promise<void> {
      const tKey = threadScopeKey(hostId, agentId);
      const wasSelected =
        !getState().isDraftNewSession &&
        getState().selectedHostId === hostId &&
        getState().selectedAgentId === agentId;

      setState(
        (s) => {
          const nextArchived = { ...s.archivedThreads };
          delete nextArchived[tKey];
          const nextTitles = { ...s.threadCustomTitles };
          delete nextTitles[tKey];
          const nextStages = { ...s.stages };
          delete nextStages[tKey];
          const nextDrafts = { ...s.composerDrafts };
          delete nextDrafts[tKey];
          const nextDirectives = { ...s.slashDirectives };
          delete nextDirectives[tKey];
          const nextInline = { ...s.inlineMentions };
          delete nextInline[tKey];
          const nextContexts = { ...s.contextItems };
          for (const ctx of nextContexts[tKey] ?? []) {
            if (
              (ctx.kind === "image" || ctx.kind === "file") &&
              ctx.previewUrl?.startsWith("blob:")
            ) {
              URL.revokeObjectURL(ctx.previewUrl);
            }
          }
          delete nextContexts[tKey];
          const nextPastes = { ...s.pastedAttachments };
          delete nextPastes[tKey];
          const nextQuotes = { ...s.diffQuotes };
          delete nextQuotes[tKey];
          const nextQueued = { ...s.queuedPrompts };
          delete nextQueued[tKey];
          const nextSideQ = { ...s.sideQuestionsByScope };
          delete nextSideQ[tKey];
          const nextSideQDrafts = { ...s.sideQuestionDrafts };
          delete nextSideQDrafts[tKey];
          const nextCreatedPrs = { ...s.sessionCreatedPrs };
          delete nextCreatedPrs[tKey];
          const nextRewound = { ...s.rewoundSessions };
          delete nextRewound[tKey];
          const nextRunningMeta =
            tKey in s.stageRunningSubagentMeta
              ? omitRecordKeys(s.stageRunningSubagentMeta, new Set([tKey]))
              : s.stageRunningSubagentMeta;
          const removedSidePanelAgents: Array<{ hostId: string; agentId: string }> = [
            { hostId, agentId },
          ];
          const removedSidePanelKeys = new Set([tKey]);
          for (const [hId, hDetail] of Object.entries(s.hostDetails)) {
            for (const ag of hDetail.agents) {
              if (ag.parent_agent_id === agentId && (ag.parent_host_id ?? hId) === hostId) {
                removedSidePanelAgents.push({ hostId: hId, agentId: ag.agent_id });
                removedSidePanelKeys.add(threadScopeKey(hId, ag.agent_id));
              }
            }
          }
          let scoped = dropSidePanelScopes(s, removedSidePanelKeys, removedSidePanelAgents);
          if (wasSelected) {
            scoped = switchSidePanelScope(scoped, `draft::${hostId}`, { dropCurrent: true });
          }
          return {
            ...scoped,
            archivedThreads: nextArchived,
            threadCustomTitles: nextTitles,
            stages: nextStages,
            stageRunningSubagentMeta: nextRunningMeta,
            composerDrafts: nextDrafts,
            slashDirectives: nextDirectives,
            inlineMentions: nextInline,
            contextItems: nextContexts,
            pastedAttachments: nextPastes,
            diffQuotes: nextQuotes,
            queuedPrompts: nextQueued,
            sideQuestionsByScope: nextSideQ,
            sideQuestionDrafts: nextSideQDrafts,
            sessionCreatedPrs: nextCreatedPrs,
            rewoundSessions: nextRewound,
            ...(wasSelected ? { selectedAgentId: null, isDraftNewSession: true } : {}),
          };
        },
        true,
      );
      draftPersistence.removeScopes([tKey], true);

      lastReportedSeenSeq.delete(tKey);
      stagePullGates.delete(tKey);
      ctx.pruneStageKeyAccess(new Set([tKey]));
      await orbitIpc.deleteAgent(hostId, agentId);
      ctx.actions.refreshSidebarView();
      notifyToast(t("store.conversationDeleted"));
    },


    setSidebarViewPrefs(patch: {
      projectSort?: SidebarProjectSort;
      threadFilter?: SidebarThreadFilter;
    }): void {
      setState(
        (s) => ({
          ...s,
          sidebarProjectSort: patch.projectSort ?? s.sidebarProjectSort,
          sidebarThreadFilter: patch.threadFilter ?? s.sidebarThreadFilter,
        }),
        true,
      );
      ctx.actions.refreshSidebarView();
    },


    reorderThreads(scopeKey: string, orderedAgentIds: string[]): void {
      if (!scopeKey || orderedAgentIds.length === 0) return;
      const prevSort = getState().sidebarProjectSort;
      setState(
        (s) => ({
          ...s,
          sidebarProjectSort: "manual",
          threadManualOrder: {
            ...s.threadManualOrder,
            [scopeKey]: orderedAgentIds,
          },
        }),
        true,
      );
      if (prevSort !== "manual") {
        ctx.actions.refreshSidebarView();
        notifyToast(t("sidebar.switchedToCustomOrder"));
      }
    },


    reorderPinnedThreads(orderedThreadKeys: string[]): void {
      if (orderedThreadKeys.length === 0) return;
      setState(
        (s) => ({
          ...s,
          threadManualOrder: {
            ...s.threadManualOrder,
            [PINNED_MANUAL_ORDER_SCOPE]: mergeSubsetOrder(
              s.threadManualOrder[PINNED_MANUAL_ORDER_SCOPE],
              orderedThreadKeys,
            ),
          },
        }),
        true,
      );
    },


    toggleSidebar(): void {
      const isCompact = getState().isCompactViewport;
      setState(
        (s) => {
          const nextOpen = !s.sidebarOpen;
          return {
            ...s,
            sidebarOpen: nextOpen,
            sidebarAutoYieldedForSidePanel: false,
            sidebarDesktopOpenPref: s.isCompactViewport ? s.sidebarDesktopOpenPref : nextOpen,
          };
        },
        !isCompact,
      );
    },


    focusSidebarFilter(): void {
      const isCompact = getState().isCompactViewport;
      setState(
        (s) => ({
          ...s,
          sidebarOpen: true,
          sidebarAutoYieldedForSidePanel: false,
          sidebarDesktopOpenPref: s.isCompactViewport ? s.sidebarDesktopOpenPref : true,
          sidebarFilterFocusNonce: s.sidebarFilterFocusNonce + 1,
        }),
        !isCompact && !getState().sidebarOpen,
      );
    },


    openStageFind(): void {
      setState(
        (s) => ({
          ...s,
          stageFindFocusNonce: s.stageFindFocusNonce + 1,
        }),
        false,
      );
    },


    updateViewportWidth(viewportWidth: number): void {
      const nextCompact = getState().isCompactViewport
        ? viewportWidth < COMPACT_VIEWPORT_EXPAND_PX
        : viewportWidth < COMPACT_VIEWPORT_COLLAPSE_PX;
      const nextTight = getState().isTightForDualSidebars
        ? viewportWidth < DUAL_SIDEBAR_RESTORE_PX
        : viewportWidth < DUAL_SIDEBAR_YIELD_PX;

      if (
        nextCompact === getState().isCompactViewport &&
        nextTight === getState().isTightForDualSidebars
      ) {
        return;
      }

      setState((s) => {
        let nextSidebarOpen = s.sidebarOpen;
        let nextYielded = s.sidebarAutoYieldedForSidePanel;
        let nextSidePanelOpen = s.sidePanelOpen;
        let nextCollapsedByResize = s.sidePanelCollapsedByResize;
        let nextUnseenTabIds = s.sidePanelUnseenTabIds;
        let nextTerminalFocused = s.terminalFocused;

        if (!s.isCompactViewport && nextCompact) {
          if (s.sidePanelOpen) {
            nextSidePanelOpen = false;
            nextCollapsedByResize = true;
            nextTerminalFocused = false;
          }
        } else if (s.isCompactViewport && !nextCompact) {
          const shouldRestoreSidePanel = s.sidePanelCollapsedByResize && !s.sidePanelUserDismissed;
          nextSidePanelOpen = s.sidePanelOpen || shouldRestoreSidePanel;
          nextCollapsedByResize = false;
          if (nextSidePanelOpen && !s.sidePanelOpen) {
            nextUnseenTabIds = EMPTY_SIDE_PANEL_UNSEEN_IDS;
          }
        }

        if (nextCompact) {
          nextSidebarOpen = false;
          nextYielded = false;
        } else if (s.isCompactViewport && !nextCompact) {
          nextSidebarOpen = nextTight && nextSidePanelOpen ? false : s.sidebarDesktopOpenPref;
          nextYielded = nextTight && nextSidePanelOpen && s.sidebarDesktopOpenPref;
        } else if (nextTight && nextSidePanelOpen && s.sidebarOpen) {
          nextSidebarOpen = false;
          nextYielded = true;
        } else if (!nextTight && s.sidebarAutoYieldedForSidePanel) {
          nextSidebarOpen = s.sidebarDesktopOpenPref;
          nextYielded = false;
        }

        return {
          ...s,
          isCompactViewport: nextCompact,
          isTightForDualSidebars: nextTight,
          sidebarOpen: nextSidebarOpen,
          sidebarAutoYieldedForSidePanel: nextYielded,
          sidePanelOpen: nextSidePanelOpen,
          sidePanelCollapsedByResize: nextCollapsedByResize,
          sidePanelUnseenTabIds: nextUnseenTabIds,
          terminalFocused: nextTerminalFocused,
        };
      });
    },


    setSidebarWidth(w: number, persist = true): void {
      const clamped = clampSidebarPrefWidth(w);
      setState((s) => (s.sidebarWidth === clamped ? s : { ...s, sidebarWidth: clamped }), persist);
    },


    resetSidebarWidth(): void {
      setState(
        (s) =>
          s.sidebarWidth === SIDEBAR_DEFAULT_WIDTH_PX
            ? s
            : { ...s, sidebarWidth: SIDEBAR_DEFAULT_WIDTH_PX },
        true,
      );
    },


    toggleHostCollapsed(hostId: string): void {
      setState((s) => ({
        ...s,
        collapsedHostIds: {
          ...s.collapsedHostIds,
          [hostId]: !s.collapsedHostIds[hostId],
        },
      }));
    },


    toggleProjectCollapsed(projectId: string): void {
      setState(
        (s) => ({
          ...s,
          collapsedProjectIds: {
            ...s.collapsedProjectIds,
            [projectId]: !s.collapsedProjectIds[projectId],
          },
        }),
        true,
      );
    },


    setAllProjectsCollapsed(projectIds: string[], collapsed: boolean): void {
      if (projectIds.length === 0) return;
      setState(
        (s) => {
          const next = { ...s.collapsedProjectIds };
          for (const id of projectIds) {
            next[id] = collapsed;
          }
          return {
            ...s,
            collapsedProjectIds: next,
          };
        },
        true,
      );
    },


    async renameProject(project: SidebarProjectGroupView, nextName: string): Promise<void> {
      const cleanName = nextName.trim();
      if (!cleanName || cleanName === project.name) return;
      try {
        const directories =
          project.directories.length > 0 ? project.directories : [project.primary_cwd];
        await orbitIpc.upsertProject({
          projectId: project.pinned_project_id ?? undefined,
          hostId: project.host_id,
          name: cleanName,
          directories,
        });
        await ctx.actions.reloadFleetOnly();
        ctx.actions.refreshSidebarView();
        notifyToast(t("store.projectRenamed", { name: cleanName }));
      } catch (err) {
        notifyToast(t("store.failedRenameProject", { detail: String(err) }));
      }
    },


    async addProjectDirectory(hostId: string, dirPath: string, name?: string): Promise<void> {
      const cleanPath = dirPath.trim();
      if (!hostId || !cleanPath) return;
      try {
        const created = await orbitIpc.upsertProject({
          hostId,
          name: (name ?? basenameFromPath(cleanPath)) || cleanPath,
          directories: [cleanPath],
        });
        const keysToUnhide = new Set<string>([projectPathScopeKey(hostId, cleanPath)]);
        if (created.directories[0]) {
          keysToUnhide.add(projectPathScopeKey(hostId, created.directories[0]));
        }
        setState(
          (s) => ({
            ...s,
            remoteFolderPickerHostId: null,
            hiddenProjectKeys: omitRecordKeys(s.hiddenProjectKeys, keysToUnhide),
            collapsedProjectIds: {
              ...s.collapsedProjectIds,
              [created.project_id]: false,
            },
          }),
          true,
        );
        await ctx.actions.reloadFleetOnly();
        ctx.actions.startNewThreadDraft(hostId, created.directories[0] ?? cleanPath);
      } catch (err) {
        notifyToast(t("store.failedAddProject", { detail: String(err) }));
      }
    },


    async promptAddProject(preferredHostId?: string): Promise<void> {
      const targetHost =
        (preferredHostId
          ? getState().fleet.hosts.find((h) => h.host_id === preferredHostId)
          : undefined) ??
        getState().fleet.hosts.find((h) => h.host_id === getState().selectedHostId) ??
        getState().fleet.hosts[0];

      if (!targetHost) {
        ctx.actions.openSettings("hosts");
        return;
      }

      if (isLocalHostCard(targetHost)) {
        const initialDir =
          getState().selectedHostId === targetHost.host_id ? getState().selectedWorkspaceCwd : null;
        try {
          const picked = await orbitIpc.pickProjectFolder(initialDir);
          if (picked) {
            await ctx.actions.addProjectDirectory(targetHost.host_id, picked);
          }
        } catch (err) {
          notifyToast(`Could not open folder picker: ${String(err)}`);
        }
        return;
      }

      // Remote host (SSH / LAN / Relay): open click-to-navigate Remote Folder Browser
      const existingListing = getState().hostDetails[targetHost.host_id]?.directory_listing;
      setState((s) => ({ ...s, remoteFolderPickerHostId: targetHost.host_id }));
      void orbitIpc.listDirectory(
        targetHost.host_id,
        existingListing?.current_path ?? null,
      );
    },


    closeRemoteFolderPicker(): void {
      setState((s) => ({ ...s, remoteFolderPickerHostId: null }));
    },


    async removePinnedProject(projectId: string): Promise<void> {
      try {
        await orbitIpc.removeProject(projectId);
        await ctx.actions.reloadFleetOnly();
      } catch (err) {
        notifyToast(t("store.failedRemoveProject", { detail: String(err) }));
      }
    },


    async deleteProject(project: SidebarProjectGroupView): Promise<void> {
      const hostId = project.host_id;
      try {
        const running = project.conversations.filter(
          ({ agent }) =>
            isAgentForegroundRunning(agent) ||
            isAgentBackgroundWaiting(agent) ||
            agent.is_awaiting_permission,
        );
        if (running.length > 0) {
          await Promise.allSettled(
            running.map(({ agent }) => orbitIpc.interruptAgent(hostId, agent.agent_id)),
          );
        }

        if (project.conversations.length > 0) {
          await Promise.allSettled(
            project.conversations.map(({ agent }) => orbitIpc.deleteAgent(hostId, agent.agent_id)),
          );
        }

        const removedPinnedIds = new Set<string>();
        if (project.pinned_project_id) {
          removedPinnedIds.add(project.pinned_project_id);
        }
        for (const pinned of getState().fleet.projects ?? []) {
          if (
            pinned.host_id === hostId &&
            pinned.directories.some((dir) => matchesProjectDirectory(project.directories, dir))
          ) {
            removedPinnedIds.add(pinned.project_id);
          }
        }
        if (removedPinnedIds.size > 0) {
          await Promise.all(
            Array.from(removedPinnedIds).map((pid) => orbitIpc.removeProject(pid)),
          );
        }

        const deletedAgentIds = new Set(project.conversations.map((c) => c.agent.agent_id));
        const deletedThreadKeys = new Set(
          project.conversations.map((c) => threadScopeKey(hostId, c.agent.agent_id)),
        );

        setState((s) => {
          const nextHidden = { ...s.hiddenProjectKeys };
          nextHidden[projectPathScopeKey(hostId, project.primary_cwd)] = true;
          for (const dir of project.directories) {
            nextHidden[projectPathScopeKey(hostId, dir)] = true;
          }

          const nextCollapsed = { ...s.collapsedProjectIds };
          delete nextCollapsed[project.id];

          const wasSelectedAgent =
            !s.isDraftNewSession &&
            s.selectedHostId === hostId &&
            s.selectedAgentId !== null &&
            deletedAgentIds.has(s.selectedAgentId);
          const wasSelectedProjectCwd =
            s.selectedHostId === hostId &&
            matchesProjectDirectory(project.directories, s.selectedWorkspaceCwd);

          let nextHostId = s.selectedHostId;
          let nextAgentId = s.selectedAgentId;
          let nextCwd = s.selectedWorkspaceCwd;
          let nextDraft = s.isDraftNewSession;

          if (wasSelectedAgent || wasSelectedProjectCwd) {
            const nextProject =
              (s.sidebarView?.projects ?? []).find(
                (p) =>
                  p.id !== project.id &&
                  !nextHidden[projectPathScopeKey(p.host_id, p.primary_cwd)] &&
                  !(
                    p.host_id === hostId &&
                    matchesProjectDirectory(project.directories, p.primary_cwd)
                  ),
              ) ?? null;

            if (nextProject) {
              nextHostId = nextProject.host_id;
              nextCwd = nextProject.primary_cwd;
              nextAgentId = null;
              nextDraft = true;
            } else {
              nextAgentId = null;
              nextCwd = "";
              nextDraft = true;
            }
          }

          let scoped = dropSidePanelScopes(
            s,
            deletedThreadKeys,
            project.conversations.map((c) => ({ hostId, agentId: c.agent.agent_id })),
          );
          if (wasSelectedAgent || wasSelectedProjectCwd) {
            const nextScopeKey = resolveSidePanelScopeKey({
              selectedHostId: nextHostId,
              selectedAgentId: nextAgentId,
              isDraftNewSession: nextDraft,
              hostDetails: s.hostDetails,
              archivedThreads: s.archivedThreads,
            });
            scoped = switchSidePanelScope(scoped, nextScopeKey, {
              dropCurrent: wasSelectedAgent || (nextDraft && !nextCwd),
            });
            if (nextDraft && nextCwd) {
              scoped = rebindDraftSidePanelCwd(
                { ...scoped, isDraftNewSession: true },
                nextCwd,
              );
            }
          }

          return {
            ...scoped,
            selectedHostId: nextHostId,
            selectedAgentId: nextAgentId,
            selectedWorkspaceCwd: nextCwd,
            selectedDiffPath:
              wasSelectedAgent || wasSelectedProjectCwd ? null : s.selectedDiffPath,
            isDraftNewSession: nextDraft,
            hiddenProjectKeys: nextHidden,
            collapsedProjectIds: nextCollapsed,
            archivedThreads: omitRecordKeys(s.archivedThreads, deletedThreadKeys),
            threadCustomTitles: omitRecordKeys(s.threadCustomTitles, deletedThreadKeys),
            stages: omitRecordKeys(s.stages, deletedThreadKeys),
            stageRunningSubagentMeta: omitRecordKeys(
              s.stageRunningSubagentMeta,
              deletedThreadKeys,
            ),
            composerDrafts: omitRecordKeys(s.composerDrafts, deletedThreadKeys),
            slashDirectives: omitRecordKeys(s.slashDirectives, deletedThreadKeys),
            inlineMentions: omitRecordKeys(s.inlineMentions, deletedThreadKeys),
            contextItems: omitRecordKeys(s.contextItems, deletedThreadKeys),
            pastedAttachments: omitRecordKeys(s.pastedAttachments, deletedThreadKeys),
            diffQuotes: omitRecordKeys(s.diffQuotes, deletedThreadKeys),
            queuedPrompts: omitRecordKeys(s.queuedPrompts, deletedThreadKeys),
            sideQuestionDrafts: omitRecordKeys(s.sideQuestionDrafts, deletedThreadKeys),
          };
        }, true);
        draftPersistence.removeScopes(deletedThreadKeys, true);
        ctx.pruneStageKeyAccess(deletedThreadKeys);

        await ctx.actions.reloadFleetOnly();
        await ctx.actions.reloadHostDetail(hostId, false);
        ctx.actions.refreshSidebarView();
        notifyToast(t("store.projectDeleted"));
      } catch (err) {
        notifyToast(t("store.failedDeleteProject", { detail: String(err) }));
        throw err;
      }
    },


    // =========================================================================
    // Git Repo Inspection, Project Reordering & Mission Workspace Actions
    // =========================================================================

    async inspectGitRepos(
      hostId: string,
      path: string,
      extraDirs?: string[],
    ): Promise<GitReposInspectionView | null> {
      if (!hostId || !path || path === "~") return null;
      try {
        const res = await orbitIpc.inspectGitRepos(hostId, path, extraDirs);
        const cacheKey = `${hostId}::${path}`;
        setState((s) => ({
          ...s,
          gitReposCache: {
            ...s.gitReposCache,
            [cacheKey]: res,
          },
        }));
        ctx.actions.refreshSidebarView();
        return res;
      } catch {
        return null;
      }
    },


    async reorderProjects(orderedKeys: string[]): Promise<void> {
      const prevSort = getState().sidebarProjectSort;
      const prevSidebarView = getState().sidebarView;
      if (prevSidebarView && orderedKeys.length > 0) {
        const rank = new Map<string, number>();
        orderedKeys.forEach((k, idx) => rank.set(k, idx));
        const projectKey = (p: (typeof prevSidebarView.projects)[number]) =>
          p.sort_order_key || p.pinned_project_id || projectPathScopeKey(p.host_id, p.primary_cwd);
        const optimisticProjects = [...prevSidebarView.projects].sort((a, b) => {
          const ra = rank.get(projectKey(a)) ?? Number.MAX_SAFE_INTEGER;
          const rb = rank.get(projectKey(b)) ?? Number.MAX_SAFE_INTEGER;
          return ra - rb;
        });
        setState(
          (s) => ({
            ...s,
            sidebarProjectSort: "manual",
            sidebarView: s.sidebarView
              ? { ...s.sidebarView, projects: optimisticProjects }
              : s.sidebarView,
          }),
          true,
        );
      }
      try {
        await orbitIpc.reorderProjects(orderedKeys);
        if (prevSort !== "manual") {
          notifyToast(t("sidebar.switchedToCustomOrder"));
        }
        ctx.actions.refreshSidebarView();
      } catch (err) {
        setState(
          (s) => ({
            ...s,
            sidebarProjectSort: prevSort,
            sidebarView: prevSidebarView ?? s.sidebarView,
          }),
          false,
        );
        notifyToast(t("store.failedReorderProjects", { detail: String(err) }));
      }
    },


    toggleWorkspaceCollapsed(workspaceId: string): void {
      setState(
        (s) => ({
          ...s,
          collapsedWorkspaceIds: {
            ...s.collapsedWorkspaceIds,
            [workspaceId]: !s.collapsedWorkspaceIds[workspaceId],
          },
        }),
        true,
      );
    },


    archiveSubagent(
      hostId: string,
      parentAgentId: string,
      target: {
        id: string;
        providerSubagentId?: string | null;
        childAgentId?: string | null;
        role?: string;
      },
    ): void {
      const keys: string[] = [];
      if (target.id) keys.push(subagentArchiveKey(hostId, parentAgentId, target.id));
      if (target.providerSubagentId) {
        keys.push(subagentArchiveKey(hostId, parentAgentId, target.providerSubagentId));
      }
      if (target.childAgentId) {
        keys.push(subagentArchiveKey(hostId, parentAgentId, target.childAgentId));
      }
      if (target.childAgentId) {
        const hasManaged = getState().hostDetails[hostId]?.agents.some(
          (a) => a.agent_id === target.childAgentId,
        );
        if (hasManaged) {
          ctx.actions.archiveThread(hostId, target.childAgentId);
        }
      }
      setState(
        (s) => {
          const nextArchived = { ...s.archivedSubagents };
          for (const k of keys) {
            nextArchived[k] = true;
          }
          const scoped = removeSubagentTabsFromScopes(s, hostId, parentAgentId, [target]);
          return {
            ...scoped,
            archivedSubagents: nextArchived,
          };
        },
        true,
      );
      if (!target.childAgentId) {
        notifyToast(
          target.role
            ? t("store.archivedNamedSubagent", { role: target.role })
            : t("store.archivedSubagent"),
        );
      }
    },


    archiveFinishedSubagents(
      hostId: string,
      parentAgentId: string,
      targets: Array<{
        id: string;
        providerSubagentId?: string | null;
        childAgentId?: string | null;
      }>,
    ): void {
      if (targets.length === 0) return;
      const managedIds = new Set<string>();
      const keys: string[] = [];
      for (const target of targets) {
        if (target.id) keys.push(subagentArchiveKey(hostId, parentAgentId, target.id));
        if (target.providerSubagentId) {
          keys.push(subagentArchiveKey(hostId, parentAgentId, target.providerSubagentId));
        }
        if (target.childAgentId) {
          keys.push(subagentArchiveKey(hostId, parentAgentId, target.childAgentId));
          if (getState().hostDetails[hostId]?.agents.some((a) => a.agent_id === target.childAgentId)) {
            managedIds.add(target.childAgentId);
          }
        }
      }
      for (const cid of managedIds) {
        ctx.actions.archiveThread(hostId, cid);
      }
      setState(
        (s) => {
          const nextArchived = { ...s.archivedSubagents };
          for (const k of keys) {
            nextArchived[k] = true;
          }
          const scoped = removeSubagentTabsFromScopes(s, hostId, parentAgentId, targets);
          return {
            ...scoped,
            archivedSubagents: nextArchived,
          };
        },
        true,
      );
      notifyToast(t("store.archivedFinishedSubagents", { count: targets.length }));
    },
  };
}
