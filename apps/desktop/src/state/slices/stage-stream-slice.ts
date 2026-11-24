import { type ComposerInlineDirective, resolveComposerSubmission } from "@/lib/slash-commands";
import {
  PROVIDERS,
  modeLabel,
} from "@/components/composer/composer-constants";
import { t as translateGlobal } from "@/lib/i18n";
import {
  hasNonEmptyScopeDraft,
} from "@/lib/draft-persistence";
import {
  defaultMimeForMedia,
  detectRichMediaKind,
  estimatePdfPageCount,
} from "@/lib/media-utils";
import {
  getCachedPromptImage,
  recordRecentSentTurnImages,
  seedPromptImagePreview,
  type SentImagePreviewMeta,
} from "@/lib/prompt-image-cache";
import {
  getCatalogForProvider,
  modelSupportsFastMode,
  resolveActiveModelSpec,
  resolveSupportedEffortForModel,
} from "@/lib/model-catalog";
import {
  matchesProjectDirectory,
} from "@/lib/projects";
import {
  type ComposerContextInput,
  type PromptBadge,
  type ProviderKind,
  type ReasoningEffort,
  type SessionMode,
  type SlashDirective,
  isAgentForegroundRunning,
  orbitIpc,
} from "@/lib/orbit-ipc";
import {
  INITIAL_STAGE_TURN_WINDOW,
  OLDER_STAGE_TURN_PAGE_SIZE,
  computeEarlierTurnSliceRequest,
  mergePrependedStageTurns,
  mergeStageDeltaWithWindow,
} from "@/lib/stage-pagination";
import {
  containsInlineMentionToken,
  extractLineSnippet,
} from "@/lib/symbol-mentions";
import {
  type InlineAiEditPromptParams,
  buildInlineAiEditPrompt,
} from "@/lib/codemirror-helpers";
import {
  MAX_ATTACHMENT_BYTES,
  basenameFromPath,
  composerScopeKey,
  isLikelyTextFile,
  isOfflineUploadError,
  resolveAvailableDraftProvider,
  slugifyPrefixedBranch,
  syncStageRunningSubagentMeta,
} from "../store-helpers";
import type { OrbitStoreContext } from "../store-runtime";
import type {
  ContextMentionItem,
  DiffQuoteSnippet,
  InlineMentionTarget,
  MediaPreviewTarget,
  PastedAttachment,
  PendingDraftCreateState,
  QueuedPromptItem,
  WindowedAgentStageView,
} from "../store-types";

export function createStageStreamSlice(ctx: OrbitStoreContext) {
  const {
    applyStageCacheUpdate,
    draftPersistence,
    emitComposerDraftChange,
    emitDraftPresenceChange,
    flushPendingSeenMarks,
    getState,
    markThreadSeenInCore,
    notifyToast,
    setState,
    stagePullGates,
    terminalChannels,
  } = ctx;

  const inflightAttachmentUploads = new Map<string, Promise<void>>();
  const submittingComposerScopes = new Set<string>();
  const loadingEarlierStageKeys = new Set<string>();
  const localQueuedDraftExtras = new Map<
    string,
    Pick<QueuedPromptItem, "slashDirective" | "contexts" | "pastes" | "quotes">
  >();

  interface ComposerDraftSnapshot {
    draft: string;
    slashDirective: ComposerInlineDirective | null;
    inlineMentions: InlineMentionTarget[];
    contextItems: ContextMentionItem[];
    pastedAttachments: PastedAttachment[];
    diffQuotes: DiffQuoteSnippet[];
  }

  function formatSubmitErrorToast(err: unknown): string {
    const detail = err instanceof Error ? err.message : String(err);
    const trimmed = detail.trim();
    const base = translateGlobal("common.actionFailed");
    return trimmed ? `${base} (${trimmed})` : base;
  }

  function captureComposerDraftSnapshot(scopeKey: string): ComposerDraftSnapshot {
    const cur = getState();
    return {
      draft: cur.composerDrafts[scopeKey] ?? "",
      slashDirective: cur.slashDirectives[scopeKey] ?? null,
      inlineMentions: [...(cur.inlineMentions[scopeKey] ?? [])],
      contextItems: [...(cur.contextItems[scopeKey] ?? [])],
      pastedAttachments: [...(cur.pastedAttachments[scopeKey] ?? [])],
      diffQuotes: [...(cur.diffQuotes[scopeKey] ?? [])],
    };
  }

  function mergeRestoredDraftText(snapshotDraft: string, currentDraft: string): string {
    if (currentDraft.trim().length > 0 && snapshotDraft.trim().length > 0) {
      return `${snapshotDraft}\n\n${currentDraft}`;
    }
    if (snapshotDraft.trim().length > 0) {
      return snapshotDraft;
    }
    return currentDraft || snapshotDraft;
  }

  function restoreComposerDraftSnapshot(
    scopeKey: string,
    snapshot: ComposerDraftSnapshot,
  ): void {
    setState((s) => {
      const currentDraft = s.composerDrafts[scopeKey] ?? "";
      const mergedDraft = mergeRestoredDraftText(snapshot.draft, currentDraft);

      const currentDirective = s.slashDirectives[scopeKey] ?? null;
      const mergedDirective = currentDirective ?? snapshot.slashDirective;

      const currentInline = s.inlineMentions[scopeKey] ?? [];
      const seenInlineTokens = new Set(currentInline.map((m) => m.token));
      const mergedInline = [
        ...snapshot.inlineMentions.filter((m) => !seenInlineTokens.has(m.token)),
        ...currentInline,
      ];

      const currentContexts = s.contextItems[scopeKey] ?? [];
      const seenContextIds = new Set(currentContexts.map((c) => c.id));
      const mergedContexts = [
        ...snapshot.contextItems.filter((c) => !seenContextIds.has(c.id)),
        ...currentContexts,
      ];

      const currentPastes = s.pastedAttachments[scopeKey] ?? [];
      const seenPasteIds = new Set(currentPastes.map((p) => p.id));
      const mergedPastes = [
        ...snapshot.pastedAttachments.filter((p) => !seenPasteIds.has(p.id)),
        ...currentPastes,
      ];

      const currentQuotes = s.diffQuotes[scopeKey] ?? [];
      const seenQuoteIds = new Set(currentQuotes.map((q) => q.id));
      const mergedQuotes = [
        ...snapshot.diffQuotes.filter((q) => !seenQuoteIds.has(q.id)),
        ...currentQuotes,
      ];

      return {
        ...s,
        composerDrafts: { ...s.composerDrafts, [scopeKey]: mergedDraft },
        slashDirectives: { ...s.slashDirectives, [scopeKey]: mergedDirective },
        inlineMentions: { ...s.inlineMentions, [scopeKey]: mergedInline },
        contextItems: { ...s.contextItems, [scopeKey]: mergedContexts },
        pastedAttachments: { ...s.pastedAttachments, [scopeKey]: mergedPastes },
        diffQuotes: { ...s.diffQuotes, [scopeKey]: mergedQuotes },
      };
    });
    draftPersistence.flushNow();
  }

  function rememberLocalQueuedDraftExtras(
    queueId: string,
    extras: Pick<QueuedPromptItem, "slashDirective" | "contexts" | "pastes" | "quotes">,
  ): void {
    localQueuedDraftExtras.set(queueId, extras);
    if (localQueuedDraftExtras.size > 32) {
      const oldest = localQueuedDraftExtras.keys().next().value;
      if (oldest) localQueuedDraftExtras.delete(oldest);
    }
  }

  function updateContextMentionItem(
    scopeKey: string,
    id: string,
    updater: (item: ContextMentionItem) => ContextMentionItem,
  ): void {
    setState((s) => ({
      ...s,
      contextItems: {
        ...s.contextItems,
        [scopeKey]: (s.contextItems[scopeKey] ?? []).map((item) =>
          item.id === id ? updater(item) : item,
        ),
      },
    }));
  }

  function removeContextMentionInScope(scopeKey: string, id: string): void {
    const removed = (getState().contextItems[scopeKey] ?? []).find((c) => c.id === id);
    if (
      removed &&
      (removed.kind === "image" || removed.kind === "file") &&
      removed.previewUrl?.startsWith("blob:")
    ) {
      URL.revokeObjectURL(removed.previewUrl);
    }
    setState((s) => ({
      ...s,
      contextItems: {
        ...s.contextItems,
        [scopeKey]: (s.contextItems[scopeKey] ?? []).filter((c) => c.id !== id),
      },
      mediaPreviewTarget:
        removed &&
        (removed.kind === "image" || removed.kind === "file") &&
        removed.previewUrl &&
        s.mediaPreviewTarget?.localUrl === removed.previewUrl
          ? null
          : s.mediaPreviewTarget,
    }));
  }

  async function uploadContextAttachmentToHost(
    scopeKey: string,
    itemId: string,
    file: File,
    filename: string,
    hostId: string,
    throwOnError: boolean,
  ): Promise<void> {
    updateContextMentionItem(scopeKey, itemId, (cur) =>
      cur.kind === "image" || cur.kind === "file" ? { ...cur, uploading: true } : cur,
    );

    const task = (async () => {
      try {
        const buf = await file.arrayBuffer();
        const staged = await orbitIpc.stageHostAttachmentBytes(
          hostId,
          filename,
          new Uint8Array(buf),
        );
        updateContextMentionItem(scopeKey, itemId, (cur) => {
          if (cur.kind === "image") {
            return {
              ...cur,
              filename: staged.filename,
              path: staged.path,
              sizeBytes: staged.size_bytes,
              stagedHostId: hostId,
              uploading: false,
            };
          }
          if (cur.kind === "file") {
            return {
              ...cur,
              path: staged.path,
              filename: staged.filename,
              sizeBytes: staged.size_bytes,
              stagedHostId: hostId,
              uploading: false,
            };
          }
          return cur;
        });
      } catch (err) {
        const msg = String(err);
        updateContextMentionItem(scopeKey, itemId, (cur) =>
          cur.kind === "image" || cur.kind === "file" ? { ...cur, uploading: false } : cur,
        );
        if (throwOnError) {
          throw err;
        }
        if (isOfflineUploadError(msg)) {
          notifyToast(translateGlobal("composer.toastHostOfflineUpload"));
        } else {
          removeContextMentionInScope(scopeKey, itemId);
          notifyToast(translateGlobal("composer.toastUploadFailed", { error: msg }));
        }
      } finally {
        inflightAttachmentUploads.delete(itemId);
      }
    })();

    inflightAttachmentUploads.set(itemId, task);
    await task;
  }

  return {


    async pullStageDelta(hostId: string, agentId: string, forceFull = false): Promise<void> {
      const tlKey = `${hostId}::${agentId}`;
      let gate = stagePullGates.get(tlKey);
      if (!gate) {
        gate = { inFlight: false, queued: false, forceFull: false };
        stagePullGates.set(tlKey, gate);
      }
      if (gate.inFlight) {
        gate.queued = true;
        gate.forceFull = gate.forceFull || forceFull;
        return;
      }

      gate.inFlight = true;
      let nextForceFull = forceFull;

      try {
        while (true) {
          gate.queued = false;
          gate.forceFull = false;

          const existing = getState().stages[tlKey];
          const sinceRevision = nextForceFull || !existing ? 0 : existing.revision;
          const sinceAuxRevision =
            nextForceFull || !existing ? null : (existing.aux_revision ?? null);
          const delta = await orbitIpc.getAgentStageDelta(
            hostId,
            agentId,
            sinceRevision,
            sinceAuxRevision,
            sinceRevision === 0 ? INITIAL_STAGE_TURN_WINDOW : null,
          );

          const prevAgent = existing?.agent ?? null;
          const nextAgent = delta.agent;
          const agentChanged =
            nextAgent !== null &&
            (prevAgent === null ||
              prevAgent.status_label !== nextAgent.status_label ||
              prevAgent.latest_seq !== nextAgent.latest_seq ||
              prevAgent.is_running !== nextAgent.is_running ||
              Boolean(prevAgent.has_background_tasks) !== Boolean(nextAgent.has_background_tasks) ||
              Boolean(prevAgent.has_armed_timers) !== Boolean(nextAgent.has_armed_timers) ||
              Boolean(prevAgent.has_running_subagents) !== Boolean(nextAgent.has_running_subagents) ||
              prevAgent.is_awaiting_permission !== nextAgent.is_awaiting_permission ||
              (prevAgent.queued_prompts?.length ?? 0) !== (nextAgent.queued_prompts?.length ?? 0) ||
              (prevAgent.queued_prompts ?? []).some(
                (q, i) => q.queue_id !== nextAgent.queued_prompts?.[i]?.queue_id,
              ) ||
              prevAgent.model !== nextAgent.model ||
              prevAgent.effort !== nextAgent.effort ||
              prevAgent.title !== nextAgent.title ||
              prevAgent.cwd !== nextAgent.cwd ||
              prevAgent.updated_at_rfc3339 !== nextAgent.updated_at_rfc3339 ||
              JSON.stringify(prevAgent.mode) !== JSON.stringify(nextAgent.mode));

          const isReset = delta.reset || sinceRevision === 0 || !existing;
          const auxChanged =
            !delta.aux_unchanged &&
            (delta.aux_revision ?? 0) !== (existing?.aux_revision ?? -1);
          const olderStateChanged =
            Boolean(delta.has_older_history) !== Boolean(existing?.has_older_history) ||
            (delta.oldest_loaded_seq ?? 0) !== (existing?.oldest_loaded_seq ?? 0);
          const hasChanges =
            isReset ||
            delta.revision !== existing.revision ||
            delta.changed_turns.length > 0 ||
            agentChanged ||
            auxChanged ||
            olderStateChanged;

          if (hasChanges) {
            let latestSeqToMark: number | null = null;
            let needsFullRetry = false;
            setState((s) => {
              const prev = s.stages[tlKey];
              if (sinceRevision > 0 && !prev && !delta.reset) {
                needsFullRetry = true;
                return s;
              }
              const resetTurns = delta.reset || sinceRevision === 0 || !prev;
              const mergedWindow = mergeStageDeltaWithWindow(prev, delta, sinceRevision);
              const reusePrevAux = Boolean(delta.aux_unchanged && !resetTurns && prev);
              const mergedStage: WindowedAgentStageView = {
                revision: delta.revision,
                host_id: delta.host_id,
                agent: delta.agent ?? prev?.agent ?? null,
                turns: mergedWindow.turns,
                window_start_turn_index: mergedWindow.window_start_turn_index,
                total_turn_count: mergedWindow.total_turn_count,
                has_older_turns:
                  mergedWindow.window_start_turn_index > 0 || Boolean(delta.has_older_history),
                has_older_history: Boolean(delta.has_older_history),
                oldest_loaded_seq: delta.oldest_loaded_seq ?? prev?.oldest_loaded_seq ?? 0,
                pending_permissions: reusePrevAux
                  ? prev.pending_permissions
                  : delta.pending_permissions,
                pending_questions: reusePrevAux
                  ? prev.pending_questions
                  : delta.pending_questions,
                session_turn_diffs: reusePrevAux
                  ? prev.session_turn_diffs
                  : delta.session_turn_diffs,
                insights: reusePrevAux ? prev.insights : delta.insights,
                latest_seq: delta.latest_seq,
                aux_revision: delta.aux_revision ?? prev?.aux_revision ?? 0,
              };
              const isViewing =
                s.workspacePanel === "conversation" &&
                !s.isDraftNewSession &&
                s.selectedHostId === hostId &&
                s.selectedAgentId === agentId;
              if (isViewing) {
                latestSeqToMark = Math.max(mergedStage.latest_seq, mergedStage.agent?.latest_seq ?? 0);
              }
              const nextStages = applyStageCacheUpdate(s, tlKey, mergedStage);
              const nextRunningMeta = syncStageRunningSubagentMeta(
                s.stageRunningSubagentMeta,
                nextStages,
                tlKey,
                mergedStage,
                reusePrevAux,
              );
              return {
                ...s,
                stages: nextStages,
                stageRunningSubagentMeta: nextRunningMeta,
              };
            });
            if (needsFullRetry) {
              gate.queued = true;
              gate.forceFull = true;
            }
            if (latestSeqToMark !== null) {
              markThreadSeenInCore(hostId, agentId, latestSeqToMark);
            }
          }

          if (!gate.queued) break;
          nextForceFull = gate.forceFull;
        }
      } finally {
        gate.inFlight = false;
      }
    },


    async loadEarlierStageTurns(
      hostId: string,
      agentId: string,
      targetTurnIndex?: number,
    ): Promise<number> {
      const tlKey = `${hostId}::${agentId}`;
      if (loadingEarlierStageKeys.has(tlKey)) {
        return 0;
      }
      const stage = getState().stages[tlKey];
      const curStart = Math.max(0, Math.floor(stage?.window_start_turn_index ?? 0));
      if (!stage) {
        return 0;
      }

      if (curStart <= 0) {
        if (!stage.has_older_history) {
          return 0;
        }
        loadingEarlierStageKeys.add(tlKey);
        try {
          const started = await orbitIpc.loadOlderAgentTimeline(hostId, agentId, 300);
          if (!started) {
            return 0;
          }
          await ctx.actions.pullStageDelta(hostId, agentId);
          return 1;
        } catch {
          return 0;
        } finally {
          loadingEarlierStageKeys.delete(tlKey);
        }
      }

      const sliceReq = computeEarlierTurnSliceRequest(
        curStart,
        OLDER_STAGE_TURN_PAGE_SIZE,
        targetTurnIndex,
      );
      if (!sliceReq) {
        return 0;
      }

      const expectedWindowStart = curStart;
      const expectedFirstTurnKey = stage.turns[0]?.key ?? null;
      loadingEarlierStageKeys.add(tlKey);

      try {
        const olderTurns = await orbitIpc.getAgentStageTurnsSlice(
          hostId,
          agentId,
          sliceReq.startTurnIndex,
          sliceReq.count,
        );
        if (!Array.isArray(olderTurns) || olderTurns.length === 0) {
          return 0;
        }

        let prependedCount = 0;
        setState((s) => {
          const prev = s.stages[tlKey];
          const merged = mergePrependedStageTurns(
            prev,
            olderTurns,
            sliceReq.startTurnIndex,
            expectedWindowStart,
            expectedFirstTurnKey,
          );
          if (!prev || !merged.applied) {
            return s;
          }
          prependedCount = merged.prependedCount;
          const nextStage: WindowedAgentStageView = {
            ...prev,
            turns: merged.turns,
            window_start_turn_index: merged.window_start_turn_index,
            total_turn_count: merged.total_turn_count,
          };
          return {
            ...s,
            stages: applyStageCacheUpdate(s, tlKey, nextStage),
          };
        });
        return prependedCount;
      } catch {
        return 0;
      } finally {
        loadingEarlierStageKeys.delete(tlKey);
      }
    },


    flushPendingSeenMarks(): void {
      flushPendingSeenMarks();
    },


    setDraftConfig(patch: {
      provider?: ProviderKind;
      acpAgentId?: string | null;
      model?: string | null;
      effort?: ReasoningEffort;
      fastMode?: boolean;
      mode?: SessionMode;
      profileId?: string | null;
      useWorktree?: boolean;
      worktreeBranch?: string;
      worktreeRepoPath?: string | null;
      workspaceId?: string | null;
    }): void {
      setState(
        (s) => {
          const nextProvider = patch.provider ?? s.draftProvider;
          const nextModels =
            patch.model !== undefined
              ? { ...s.draftModels, [nextProvider]: patch.model }
              : s.draftModels;
          return {
            ...s,
            draftProvider: nextProvider,
            draftAcpAgentId:
              patch.acpAgentId !== undefined ? patch.acpAgentId : s.draftAcpAgentId,
            draftModels: nextModels,
            draftEffort: patch.effort ?? s.draftEffort,
            draftFastMode: patch.fastMode ?? s.draftFastMode,
            draftMode: patch.mode ?? s.draftMode,
            draftProfileId:
              patch.profileId !== undefined
                ? patch.profileId
                : patch.provider !== undefined || patch.model !== undefined
                  ? null
                  : s.draftProfileId,
            draftUseWorktree: patch.useWorktree ?? s.draftUseWorktree,
            draftWorktreeBranch: patch.worktreeBranch ?? s.draftWorktreeBranch,
            draftWorktreeRepoPath:
              patch.worktreeRepoPath !== undefined
                ? patch.worktreeRepoPath
                : patch.useWorktree === false
                  ? null
                  : s.draftWorktreeRepoPath,
            draftWorkspaceId:
              patch.workspaceId !== undefined ? patch.workspaceId : s.draftWorkspaceId,
          };
        },
        true,
      );
    },


    toggleActiveFastMode(action: "toggle" | "enable" | "disable" | boolean = "toggle"): void {
      const hostId = getState().selectedHostId;
      const detail = hostId ? getState().hostDetails[hostId] : undefined;
      const activeAgent =
        !getState().isDraftNewSession && hostId && getState().selectedAgentId
          ? detail?.agents.find((a) => a.agent_id === getState().selectedAgentId)
          : undefined;

      const availableProviders = detail?.host.available_providers ?? [];
      const draftProvider: ProviderKind =
        availableProviders.length > 0 && !availableProviders.includes(getState().draftProvider)
          ? (PROVIDERS.find((p) => availableProviders.includes(p.id))?.id ?? availableProviders[0])
          : getState().draftProvider;

      const activeProvider: ProviderKind = activeAgent ? activeAgent.provider : draftProvider;
      const rawModel = activeAgent ? activeAgent.model : (getState().draftModels[activeProvider] ?? null);
      const cat = getCatalogForProvider(detail?.provider_catalogs, activeProvider);
      const spec = resolveActiveModelSpec(cat, rawModel);
      const modelAllowsFast = modelSupportsFastMode(activeProvider, spec);
      const isDraft = !activeAgent;

      if (!modelAllowsFast && (isDraft || !activeAgent.fast_mode)) {
        if (isDraft && (action === false || action === "disable" || getState().draftFastMode)) {
          ctx.actions.setDraftConfig({ fastMode: false });
        }
        notifyToast(translateGlobal("composer.toastFastUnavailable"));
        return;
      }

      const current = activeAgent ? Boolean(activeAgent.fast_mode) : getState().draftFastMode;

      const next =
        action === true || action === "enable"
          ? true
          : action === false || action === "disable"
            ? false
            : !current;

      if (next && !modelAllowsFast) {
        notifyToast(translateGlobal("composer.toastFastUnavailable"));
        return;
      }

      if (activeAgent && hostId) {
        void orbitIpc
          .updateAgent({
            hostId,
            agentId: activeAgent.agent_id,
            fastMode: next,
          })
          .catch((err) => {
            notifyToast(translateGlobal("composer.modelConfigFailed", { error: String(err) }));
          });
      } else {
        ctx.actions.setDraftConfig({ fastMode: next });
      }
      notifyToast(
        next ? translateGlobal("composer.toastFastOn") : translateGlobal("composer.toastFastOff"),
      );
    },


    cycleActiveMode(): void {
      const cycle: SessionMode[] = ["default", "auto", "accept_edits", "full_access"];
      const snap = getState();
      if (snap.isDraftNewSession || !snap.selectedHostId || !snap.selectedAgentId) {
        const idx = cycle.findIndex((m) => m === snap.draftMode);
        const next = cycle[(idx + 1) % cycle.length];
        ctx.actions.setDraftConfig({ mode: next });
        notifyToast(
          translateGlobal("composer.modeSwitchedToast", {
            mode: modeLabel(next, translateGlobal),
          }),
        );
        return;
      }
      const detail = snap.hostDetails[snap.selectedHostId];
      const ag = detail?.agents.find((a) => a.agent_id === snap.selectedAgentId);
      if (!ag) return;
      const idx = cycle.findIndex((m) => m === ag.mode);
      const next = cycle[(idx + 1) % cycle.length];
      void orbitIpc
        .setSessionMode(snap.selectedHostId, ag.agent_id, next)
        .then(() => {
          notifyToast(
            translateGlobal("composer.modeSwitchedToast", {
              mode: modeLabel(next, translateGlobal),
            }),
          );
        })
        .catch((err) => {
          notifyToast(translateGlobal("composer.modeSwitchFailed", { error: String(err) }));
        });
    },


    setComposerDraft(text: string): void {
      const key = composerScopeKey(getState());
      if ((getState().composerDrafts[key] ?? "") === text) return;
      const prev = getState();
      // Update draft slice without broadcasting global `emitChange()` across unrelated components
      ctx.replaceStateSilent({
        ...getState(),
        composerDrafts: {
          ...getState().composerDrafts,
          [key]: text,
        },
      });
      const hadDraft = hasNonEmptyScopeDraft(prev, key);
      const hasDraft = hasNonEmptyScopeDraft(getState(), key);
      if (hasDraft) {
        draftPersistence.markDirty(key);
      } else {
        draftPersistence.removeScopes([key], false);
      }
      if (hadDraft !== hasDraft) {
        emitDraftPresenceChange();
      }
      emitComposerDraftChange();
    },


    setSlashDirective(directive: ComposerInlineDirective | null): void {
      const key = composerScopeKey(getState());
      setState((s) => ({
        ...s,
        slashDirectives: {
          ...s.slashDirectives,
          [key]: directive,
        },
      }));
    },


    addPastedAttachment(content: string): void {
      const key = composerScopeKey(getState());
      const lineCount = content.split("\n").length;
      const item: PastedAttachment = {
        id: `paste_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`,
        label: translateGlobal("composer.pastedTextLabel", { count: lineCount }),
        lineCount,
        content,
      };
      setState((s) => ({
        ...s,
        pastedAttachments: {
          ...s.pastedAttachments,
          [key]: [...(s.pastedAttachments[key] ?? []), item],
        },
      }));
    },


    removePastedAttachment(id: string): void {
      const key = composerScopeKey(getState());
      setState((s) => ({
        ...s,
        pastedAttachments: {
          ...s.pastedAttachments,
          [key]: (s.pastedAttachments[key] ?? []).filter((p) => p.id !== id),
        },
      }));
    },


    openMediaPreview(target: MediaPreviewTarget): void {
      setState((s) => ({ ...s, mediaPreviewTarget: target }));
    },


    closeMediaPreview(): void {
      setState((s) => ({ ...s, mediaPreviewTarget: null }));
    },


    insertCitationIntoComposer(citation: string): void {
      const clean = citation.trim();
      if (!clean) return;
      const key = composerScopeKey(getState());
      const current = getState().composerDrafts[key] ?? "";
      const sep = current.length === 0 || /\s$/.test(current) ? "" : " ";
      const next = `${current}${sep}\`${clean}\` `;
      setState((s) => ({
        ...s,
        composerDrafts: { ...s.composerDrafts, [key]: next },
      }));
      draftPersistence.markDirty(key);
      notifyToast(translateGlobal("composer.toastCited", { citation: clean }));
      ctx.actions.focusComposer();
    },


    quoteDiffLineToComposer(path: string, lineNumber: number | null, code: string): void {
      const key = composerScopeKey(getState());
      const item: DiffQuoteSnippet = {
        id: `quote_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`,
        path,
        lineNumber,
        code,
      };
      setState((s) => ({
        ...s,
        diffQuotes: {
          ...s.diffQuotes,
          [key]: [...(s.diffQuotes[key] ?? []), item],
        },
      }));
      ctx.actions.refreshSidebarView();
      ctx.actions.focusComposer();
    },


    appendToComposerDraft(text: string): void {
      const clean = text.trim();
      if (!clean) return;
      const key = composerScopeKey(getState());
      setState((s) => {
        const existing = (s.composerDrafts[key] ?? "").trimEnd();
        const nextText = existing ? `${existing}\n\n${clean}` : clean;
        return {
          ...s,
          composerDrafts: {
            ...s.composerDrafts,
            [key]: nextText,
          },
        };
      });
      ctx.actions.refreshSidebarView();
    },


    async dispatchInlineCodeEdit(
      params: InlineAiEditPromptParams,
    ): Promise<"sent" | "queued" | "composer"> {
      const built = buildInlineAiEditPrompt(params);
      const state = getState();
      const hostId = state.selectedHostId;
      const agentId = state.selectedAgentId;

      if (!hostId || state.isDraftNewSession || !agentId) {
        const key = composerScopeKey(state);
        setState((s) => {
          const existing = (s.composerDrafts[key] ?? "").trimEnd();
          const nextText = existing ? `${existing}\n\n${built.wirePrompt}` : built.wirePrompt;
          return {
            ...s,
            composerDrafts: {
              ...s.composerDrafts,
              [key]: nextText,
            },
          };
        });
        ctx.actions.refreshSidebarView();
        ctx.actions.focusComposer();
        notifyToast(translateGlobal("sidePanel.editorAiEditInsertedComposer"));
        return "composer";
      }

      const detail = state.hostDetails[hostId];
      const activeAgent = detail?.agents.find((a) => a.agent_id === agentId);
      const supportsServerQueue = Boolean(detail?.host.supports_server_queue);

      if (isAgentForegroundRunning(activeAgent) || activeAgent?.is_awaiting_permission) {
        if (supportsServerQueue) {
          const queueId = `q_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
          await orbitIpc.enqueuePrompt(hostId, agentId, built.wirePrompt, {
            displayPrompt: built.displayPrompt,
            queueId,
          });
        } else {
          const key = composerScopeKey(state);
          const queuedItem: QueuedPromptItem = {
            id: `q_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`,
            wirePrompt: built.wirePrompt,
            displayText: built.displayPrompt,
            badges: [],
            slashDirective: null,
            contexts: [],
            pastes: [],
            quotes: [],
          };
          setState((s) => ({
            ...s,
            queuedPrompts: {
              ...s.queuedPrompts,
              [key]: [...(s.queuedPrompts[key] ?? []), queuedItem],
            },
          }));
        }
        notifyToast(
          translateGlobal("sidePanel.editorAiEditQueued", { range: built.rangeLabel }),
        );
        return "queued";
      }

      setState((s) =>
        s.selectedHostId === hostId && !s.isDraftNewSession && s.selectedAgentId === agentId
          ? { ...s, sessionScrollNonce: s.sessionScrollNonce + 1 }
          : s,
      );
      await orbitIpc.sendPrompt(hostId, agentId, built.wirePrompt, {
        displayPrompt: built.displayPrompt,
      });
      notifyToast(
        translateGlobal("sidePanel.editorAiEditDispatched", { range: built.rangeLabel }),
      );
      return "sent";
    },


    removeDiffQuote(id: string): void {
      const key = composerScopeKey(getState());
      setState((s) => ({
        ...s,
        diffQuotes: {
          ...s.diffQuotes,
          [key]: (s.diffQuotes[key] ?? []).filter((q) => q.id !== id),
        },
      }));
    },


    async ensureWorkspaceFiles(hostId: string, cwd: string, force = false): Promise<void> {
      if (!hostId || !cwd || cwd === "~") return;
      const cacheKey = `${hostId}::${cwd}`;
      if (!force && (getState().workspaceFilesCache[cacheKey]?.length ?? 0) > 0) return;
      const files = await orbitIpc.getWorkspaceFiles(hostId, cwd, force);
      if (files.length > 0 || force) {
        setState((s) => ({
          ...s,
          workspaceFilesCache: {
            ...s.workspaceFilesCache,
            [cacheKey]: files,
          },
        }));
      }
    },


    addContextMention(item: ContextMentionItem): void {
      const key = composerScopeKey(getState());
      const existing = getState().contextItems[key] ?? [];
      // Prevent duplicate file, git_diff, or skill mentions
      const duplicate = existing.some((x) => {
        if (x.kind === "file" && item.kind === "file") {
          return x.path.length > 0 && x.path === item.path;
        }
        if (x.kind === "git_diff" && item.kind === "git_diff") return x.staged === item.staged;
        if (x.kind === "skill" && item.kind === "skill") {
          return x.slug.toLowerCase() === item.slug.toLowerCase();
        }
        return false;
      });
      if (duplicate) return;

      setState((s) => ({
        ...s,
        contextItems: {
          ...s.contextItems,
          [key]: [...(s.contextItems[key] ?? []), item],
        },
      }));
    },


    removeContextMention(id: string): void {
      removeContextMentionInScope(composerScopeKey(getState()), id);
    },


    registerInlineMention(target: InlineMentionTarget): void {
      const key = composerScopeKey(getState());
      setState((s) => {
        const prev = s.inlineMentions[key] ?? [];
        const filtered =
          target.kind === "code_symbol"
            ? prev.filter(
                (m) =>
                  !(
                    m.kind === "code_symbol" &&
                    m.path === target.path &&
                    m.symbolName === target.symbolName &&
                    m.startLine === target.startLine
                  ),
              )
            : prev.filter((m) => m.token !== target.token);
        return {
          ...s,
          inlineMentions: {
            ...s.inlineMentions,
            [key]: [...filtered, target],
          },
        };
      });
    },


    captureActiveTerminalTail(): { label: string; lineCount: number; content: string } | null {
      const hostId = getState().selectedHostId;
      if (!hostId) return null;
      const activeTab =
        getState().terminalTabs.find(
          (t) => t.hostId === hostId && t.key === getState().activeTerminalTabKey,
        ) ?? getState().terminalTabs.find((t) => t.hostId === hostId);
      const slot = activeTab?.slot ?? 1;
      const ch = terminalChannels.get(`${hostId}::${slot}`);
      if (!ch || ch.chunks.length === 0) {
        notifyToast(translateGlobal("composer.toastNoTerminalOutput"));
        return null;
      }

      const decoder = new TextDecoder();
      const rawCombined = ch.chunks.map((b) => decoder.decode(b, { stream: true })).join("");
      const stripped = rawCombined.replace(
        // eslint-disable-next-line no-control-regex
        /[\u001b\u009b][[()#;?]*(?:[0-9]{1,4}(?:;[0-9]{0,4})*)?[0-9A-ORZcf-nqry=><]/g,
        "",
      );
      const tailLines = stripped
        .split(/\r?\n/)
        .map((l) => l.trimEnd())
        .filter((l) => l.length > 0)
        .slice(-80);

      if (tailLines.length === 0) {
        notifyToast(translateGlobal("composer.toastEmptyTerminalBuffer"));
        return null;
      }

      return {
        label: translateGlobal("composer.terminalLinesLabel", {
          title: activeTab?.title ?? translateGlobal("sidePanel.actionTerminal"),
          count: tailLines.length,
        }),
        lineCount: tailLines.length,
        content: tailLines.join("\n"),
      };
    },


    attachActiveTerminalOutput(): boolean {
      const snap = ctx.actions.captureActiveTerminalTail();
      if (!snap) return false;
      ctx.actions.addContextMention({
        id: `term_${Date.now()}`,
        kind: "terminal_output",
        label: snap.label,
        lineCount: snap.lineCount,
        content: snap.content,
      });
      return true;
    },


    async stageAndAttachFile(file: File): Promise<void> {
      if (file.size > MAX_ATTACHMENT_BYTES) {
        notifyToast(translateGlobal("composer.toastAttachmentTooLarge", { name: file.name }));
        return;
      }

      const scopeKey = composerScopeKey(getState());
      const hostId = getState().selectedHostId;
      const isImage = file.type.startsWith("image/");

      if (isImage) {
        const previewUrl = URL.createObjectURL(file);
        const name = file.name || `screenshot-${Date.now()}.png`;
        const id = `img_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`;
        ctx.actions.addContextMention({
          id,
          kind: "image",
          filename: name,
          path: "",
          previewUrl,
          sizeBytes: file.size,
          localFile: file,
          uploading: Boolean(hostId),
        });
        if (hostId) {
          await uploadContextAttachmentToHost(scopeKey, id, file, name, hostId, false);
        } else {
          notifyToast(translateGlobal("composer.toastSelectHostUpload"));
        }
        return;
      }

      const buf = await file.arrayBuffer();
      const rawBytes = new Uint8Array(buf);
      const mediaKind = detectRichMediaKind(file.name, file.type);

      if (mediaKind) {
        const name = file.name || `attachment-${Date.now()}.${mediaKind === "pdf" ? "pdf" : mediaKind === "audio" ? "mp3" : "mp4"}`;
        const id = `${mediaKind}_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`;
        const mimeType = file.type || defaultMimeForMedia(name, mediaKind);
        const previewUrl = URL.createObjectURL(file);
        const pdfPageCount = mediaKind === "pdf" ? estimatePdfPageCount(rawBytes) : null;
        ctx.actions.addContextMention({
          id,
          kind: "file",
          path: "",
          filename: name,
          sizeBytes: file.size,
          localFile: file,
          uploading: Boolean(hostId),
          mediaKind,
          mimeType,
          pdfPageCount,
          previewUrl,
        });
        if (hostId) {
          await uploadContextAttachmentToHost(scopeKey, id, file, name, hostId, false);
        } else {
          notifyToast(translateGlobal("composer.toastSelectHostUpload"));
        }
        return;
      }

      // Text / code files <= 64 KiB without NUL bytes: inline into pastedAttachments
      if (isLikelyTextFile(file, rawBytes)) {
        const text = new TextDecoder().decode(rawBytes);
        const lineCount = text.split("\n").length;
        setState((s) => ({
          ...s,
          pastedAttachments: {
            ...s.pastedAttachments,
            [scopeKey]: [
              ...(s.pastedAttachments[scopeKey] ?? []),
              {
                id: `file_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`,
                label: translateGlobal("composer.fileLinesLabel", {
                  name: file.name,
                  count: lineCount,
                }),
                filename: file.name,
                lineCount,
                content: `// File: ${file.name}\n${text}`,
              },
            ],
          },
        }));
        return;
      }

      const name = file.name || `attachment-${Date.now()}.bin`;
      const id = `fref_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`;
      ctx.actions.addContextMention({
        id,
        kind: "file",
        path: "",
        filename: name,
        sizeBytes: file.size,
        localFile: file,
        uploading: Boolean(hostId),
      });
      if (hostId) {
        await uploadContextAttachmentToHost(scopeKey, id, file, name, hostId, false);
      } else {
        notifyToast(translateGlobal("composer.toastSelectHostUpload"));
      }
    },


    async submitSlashCompact(extraFocus = ""): Promise<void> {
      const hostId = getState().selectedHostId;
      const agentId = getState().selectedAgentId;
      if (!hostId || getState().isDraftNewSession || !agentId) {
        notifyToast(translateGlobal("composer.toastSessionRequiredCompact"));
        return;
      }
      const detail = getState().hostDetails[hostId];
      const activeAgent = detail?.agents.find((a) => a.agent_id === agentId);
      const provider = activeAgent?.provider ?? getState().draftProvider;
      const cwd = activeAgent?.cwd || getState().selectedWorkspaceCwd || "~";
      const composed = await orbitIpc.composePrompt({
        provider,
        raw_text: extraFocus.trim(),
        explicit_directive: "compact",
        host_id: hostId,
        cwd,
        contexts: [],
        quotes: [],
        pastes: [],
      });

      await orbitIpc.sendPrompt(hostId, agentId, composed.wire_prompt, {
        displayPrompt: composed.display_prompt,
        badges: composed.badges,
      });
    },


    async submitComposer(steerImmediately = false): Promise<void> {
      const state = getState();
      const hostId = state.selectedHostId;
      if (!hostId) return;

      const key = composerScopeKey(state);
      const rawDraft = (state.composerDrafts[key] ?? "").trim();
      const submission = resolveComposerSubmission(state.slashDirectives[key] ?? null, rawDraft);

      // Local commands must remain available even while a main prompt is uploading or queued.
      if (submission.kind !== "prompt") {
        const clearDirectiveDraft = () => {
          setState((s) => ({
            ...s,
            composerDrafts: { ...s.composerDrafts, [key]: "" },
            slashDirectives: { ...s.slashDirectives, [key]: null },
          }));
          draftPersistence.flushNow();
        };
        if (submission.kind === "btw") {
          clearDirectiveDraft();
          ctx.actions.openSideQuestionTab();
          if (submission.text) void ctx.actions.submitSideQuestion(submission.text);
          return;
        }

        const agentId = state.selectedAgentId;
        if (state.isDraftNewSession || !agentId || !state.hostDetails[hostId]?.agents.some((a) => a.agent_id === agentId)) {
          notifyToast(translateGlobal("composer.toastSessionRequiredGoal"));
          return;
        }
        if (!submission.text) {
          notifyToast(translateGlobal("composer.toastGoalObjectiveRequired"));
          ctx.actions.focusComposer();
          return;
        }
        const prevGoalDraft = getState().composerDrafts[key] ?? "";
        const prevGoalDirective = getState().slashDirectives[key] ?? null;
        clearDirectiveDraft();
        try {
          await ctx.actions.startGoalRun({ hostId, agentId, objective: submission.text });
        } catch (err) {
          setState((s) => ({
            ...s,
            composerDrafts: {
              ...s.composerDrafts,
              [key]: mergeRestoredDraftText(prevGoalDraft, s.composerDrafts[key] ?? ""),
            },
            slashDirectives: {
              ...s.slashDirectives,
              [key]: s.slashDirectives[key] ?? prevGoalDirective,
            },
          }));
          draftPersistence.flushNow();
          notifyToast(formatSubmitErrorToast(err));
        }
        return;
      }

      if (submittingComposerScopes.has(key)) return;
      submittingComposerScopes.add(key);
      try {
        await ctx.actions._submitComposerUnlocked(hostId, key, steerImmediately, rawDraft, submission.directive);
      } finally {
        submittingComposerScopes.delete(key);
      }
    },


    async _submitComposerUnlocked(
      hostId: string,
      key: string,
      steerImmediately: boolean,
      rawDraft: string,
      storedDirective: SlashDirective | null,
    ): Promise<void> {
      // Snapshot ALL routing and draft config fields up front before any async upload/diff/compose
      // awaits so switching Agent or Workspace mid-submit can never misroute the prompt or settings.
      const initialHostDetail = getState().hostDetails[hostId];
      const initialIsDraft = getState().isDraftNewSession || !getState().selectedAgentId;
      const initialAgentId = initialIsDraft ? null : getState().selectedAgentId;
      const initialActiveAgent = initialAgentId
        ? (initialHostDetail?.agents.find((a) => a.agent_id === initialAgentId) ?? null)
        : null;
      const submitTarget = {
        hostId,
        scopeKey: key,
        isDraftNewSession: initialIsDraft,
        agentId: initialAgentId,
        workspaceCwd: getState().selectedWorkspaceCwd || "~",
        draftProvider: getState().draftProvider,
        draftAcpAgentId: getState().draftAcpAgentId,
        draftModels: { ...getState().draftModels },
        draftEffort: getState().draftEffort,
        draftFastMode: getState().draftFastMode,
        draftMode: getState().draftMode,
        draftProfileId: getState().draftProfileId,
        draftWorkspaceId: getState().draftWorkspaceId,
        draftUseWorktree: getState().draftUseWorktree,
        draftWorktreeBranch: getState().draftWorktreeBranch,
        draftWorktreeRepoPath: getState().draftWorktreeRepoPath,
        worktreeBranchPrefix: getState().worktreeBranchPrefix,
        providerForCompose: initialActiveAgent
          ? initialActiveAgent.provider
          : resolveAvailableDraftProvider(initialHostDetail, getState().draftProvider),
      };

      const initialContexts = getState().contextItems[submitTarget.scopeKey] ?? [];
      const registeredInline = getState().inlineMentions[submitTarget.scopeKey] ?? [];
      const pastes = getState().pastedAttachments[submitTarget.scopeKey] ?? [];
      const quotes = getState().diffQuotes[submitTarget.scopeKey] ?? [];

      // Intercept standalone immediate UI/mode slash commands typed & submitted directly via Enter
      // so they never leak as literal `/ask` / `/diff` / `/new` prompts to the LLM.
      if (
        !storedDirective &&
        initialContexts.length === 0 &&
        pastes.length === 0 &&
        quotes.length === 0 &&
        /^\/[a-z0-9_-]+$/i.test(rawDraft)
      ) {
        const standaloneCmd = rawDraft.toLowerCase();
        const clearBareCommand = () => {
          setState((s) => ({
            ...s,
            composerDrafts: { ...s.composerDrafts, [submitTarget.scopeKey]: "" },
          }));
          draftPersistence.flushNow();
        };
        const applyImmediateMode = async (nextMode: SessionMode) => {
          clearBareCommand();
          if (submitTarget.isDraftNewSession || !submitTarget.agentId) {
            ctx.actions.setDraftConfig({ mode: nextMode });
            notifyToast(
              translateGlobal("composer.modeSwitchedToast", {
                mode: modeLabel(nextMode, translateGlobal),
              }),
            );
          } else {
            try {
              await orbitIpc.setSessionMode(submitTarget.hostId, submitTarget.agentId, nextMode);
              notifyToast(
                translateGlobal("composer.modeSwitchedToast", {
                  mode: modeLabel(nextMode, translateGlobal),
                }),
              );
            } catch (err) {
              notifyToast(translateGlobal("composer.modeSwitchFailed", { error: String(err) }));
            }
          }
        };
        if (standaloneCmd === "/ask" || standaloneCmd === "/supervised") {
          await applyImmediateMode("default");
          return;
        }
        if (standaloneCmd === "/auto") {
          await applyImmediateMode("auto");
          return;
        }
        if (standaloneCmd === "/edits") {
          await applyImmediateMode("accept_edits");
          return;
        }
        if (standaloneCmd === "/full") {
          await applyImmediateMode("full_access");
          return;
        }
        if (standaloneCmd === "/diff") {
          clearBareCommand();
          ctx.actions.toggleReviewTab();
          return;
        }
        if (standaloneCmd === "/files") {
          clearBareCommand();
          ctx.actions.openFilesExplorerTab();
          return;
        }
        if (standaloneCmd === "/browser") {
          clearBareCommand();
          ctx.actions.openBrowserTab();
          return;
        }
        if (standaloneCmd === "/terminal") {
          clearBareCommand();
          ctx.actions.toggleTerminalTab();
          return;
        }
        if (standaloneCmd === "/new") {
          clearBareCommand();
          ctx.actions.startNewThreadDraft();
          return;
        }
        if (standaloneCmd === "/settings") {
          clearBareCommand();
          ctx.actions.openSettings();
          return;
        }
        if (standaloneCmd === "/export") {
          if (!submitTarget.isDraftNewSession && submitTarget.agentId) {
            clearBareCommand();
            ctx.actions.openExportDialog(submitTarget.hostId, submitTarget.agentId);
          } else {
            notifyToast(translateGlobal("composer.toastSessionRequiredExport"));
          }
          return;
        }
        if (standaloneCmd === "/copy") {
          if (!submitTarget.isDraftNewSession && submitTarget.agentId) {
            clearBareCommand();
            void ctx.actions.openCopyCodeBlocksModal(submitTarget.hostId, submitTarget.agentId);
          } else {
            notifyToast(translateGlobal("composer.toastSessionRequiredCopy"));
          }
          return;
        }
      }

      // Ensure every attached image/file has completed uploading to the target `submitTarget.hostId`
      // BEFORE composing the wire prompt or clearing the composer draft.
      if (initialContexts.some((ctx) => ctx.kind === "image" || ctx.kind === "file")) {
        try {
          const pendingPromises: Promise<void>[] = [];
          for (const ctx of initialContexts) {
            const inflight = inflightAttachmentUploads.get(ctx.id);
            if (inflight) {
              pendingPromises.push(inflight);
            }
          }
          if (pendingPromises.length > 0) {
            await Promise.all(pendingPromises);
          }

          const refreshedContexts = getState().contextItems[submitTarget.scopeKey] ?? [];
          for (const ctx of refreshedContexts) {
            if (
              (ctx.kind === "image" || ctx.kind === "file") &&
              ctx.localFile &&
              (ctx.stagedHostId !== submitTarget.hostId || !ctx.path)
            ) {
              const fname = ctx.filename || ctx.localFile.name || "attachment.bin";
              await uploadContextAttachmentToHost(
                submitTarget.scopeKey,
                ctx.id,
                ctx.localFile,
                fname,
                submitTarget.hostId,
                true,
              );
            }
          }
        } catch (err) {
          notifyToast(translateGlobal("composer.toastUploadFailed", { error: String(err) }));
          return;
        }
      }

      const existingContexts = getState().contextItems[submitTarget.scopeKey] ?? [];

      // Resolve any inline `@tokens` still present in `rawDraft` without showing attachment boxes
      const resolvedInlineContexts: ContextMentionItem[] = [];
      for (const m of registeredInline) {
        if (!containsInlineMentionToken(rawDraft, m.token)) continue;
        if (m.kind === "file") {
          resolvedInlineContexts.push({ id: `inl_${m.path}`, kind: "file", path: m.path });
        } else if (m.kind === "git_diff") {
          resolvedInlineContexts.push({
            id: `inl_diff_${m.staged ? "s" : "w"}`,
            kind: "git_diff",
            staged: m.staged,
            label: m.label,
          });
        } else if (m.kind === "terminal_output") {
          resolvedInlineContexts.push({
            id: "inl_term",
            kind: "terminal_output",
            label: m.label,
            lineCount: m.lineCount,
            content: m.content,
          });
        } else if (m.kind === "code_symbol") {
          resolvedInlineContexts.push({
            id: `inl_sym_${m.path}_${m.symbolName}_${m.startLine}`,
            kind: "code_symbol",
            path: m.path,
            startLine: m.startLine,
            endLine: m.endLine,
            symbolName: m.symbolName,
            symbolKind: m.symbolKind,
            signature: m.signature ?? null,
            container: m.container ?? null,
            language: m.language,
          });
        }
      }

      const mergedContexts: ContextMentionItem[] = [...existingContexts];
      for (const rc of resolvedInlineContexts) {
        const dup = mergedContexts.some((x) => {
          if (x.kind === "file" && rc.kind === "file") return x.path === rc.path;
          if (x.kind === "git_diff" && rc.kind === "git_diff") return x.staged === rc.staged;
          if (x.kind === "terminal_output" && rc.kind === "terminal_output") return true;
          if (x.kind === "code_symbol" && rc.kind === "code_symbol") {
            return (
              x.path === rc.path &&
              x.symbolName === rc.symbolName &&
              x.startLine === rc.startLine
            );
          }
          return false;
        });
        if (!dup) mergedContexts.push(rc);
      }

      const cwd = submitTarget.workspaceCwd;

      // Resolve line-numbered code snippets for any cited `@symbol` mentions (bounded & file-deduplicated)
      const symbolContexts = mergedContexts.filter(
        (c): c is Extract<ContextMentionItem, { kind: "code_symbol" }> =>
          c.kind === "code_symbol" && !c.snippet,
      );
      const resolvedSymbolSnippets = new Map<string, string>();
      if (symbolContexts.length > 0 && cwd && cwd !== "~") {
        const uniquePaths = Array.from(new Set(symbolContexts.map((s) => s.path))).slice(0, 8);
        const fileTextByPath = new Map<string, string>();
        await Promise.all(
          uniquePaths.map(async (relPath) => {
            try {
              const view = await orbitIpc.readWorkspaceFile(submitTarget.hostId, cwd, relPath);
              if (view.content_kind === "text" && view.content) {
                fileTextByPath.set(relPath, view.content);
              }
            } catch {
              // Gracefully fall back to signature + path:lines metadata if file read fails
            }
          }),
        );
        for (const sym of symbolContexts) {
          const text = fileTextByPath.get(sym.path);
          if (!text) continue;
          const snippet = extractLineSnippet(text, sym.startLine, sym.endLine, 6000);
          if (snippet) {
            resolvedSymbolSnippets.set(sym.id, snippet);
          }
        }
      }
      const needsWorkingDiff =
        storedDirective === "review" ||
        (!storedDirective && /^\/review(?:\s|$)/i.test(rawDraft)) ||
        mergedContexts.some((c) => c.kind === "git_diff" && !c.staged);
      const needsStagedDiff = mergedContexts.some((c) => c.kind === "git_diff" && c.staged);

      let workingDiffText: string | null = null;
      let stagedDiffText: string | null = null;
      let composed: Awaited<ReturnType<typeof orbitIpc.composePrompt>>;
      try {
        if (needsWorkingDiff) {
          await orbitIpc.fetchGitDiff(submitTarget.hostId, cwd, null, false);
          const gr = await orbitIpc.getGitReview(submitTarget.hostId, cwd, null);
          if (gr.raw_diff.trim()) workingDiffText = gr.raw_diff;
        }
        if (needsStagedDiff) {
          await orbitIpc.fetchGitDiff(submitTarget.hostId, cwd, null, true);
          const gr = await orbitIpc.getGitReview(submitTarget.hostId, cwd, null);
          if (gr.raw_diff.trim()) stagedDiffText = gr.raw_diff;
        }

        const providerForCompose: ProviderKind = submitTarget.providerForCompose;

        const nonSkillContexts = mergedContexts.filter((c) => c.kind !== "skill");

        const contextInputs: ComposerContextInput[] = nonSkillContexts.map((ctx) => {
          switch (ctx.kind) {
            case "image":
              return { kind: "image", path: ctx.path, filename: ctx.filename };
            case "file": {
              if (ctx.stagedHostId) {
                const fname = ctx.filename || ctx.path;
                if (ctx.mediaKind === "pdf") {
                  return { kind: "pdf", path: ctx.path, filename: fname };
                }
                if (ctx.mediaKind === "audio") {
                  return { kind: "audio", path: ctx.path, filename: fname };
                }
                if (ctx.mediaKind === "video") {
                  return { kind: "video", path: ctx.path, filename: fname };
                }
                return {
                  kind: "staged_file",
                  path: ctx.path,
                  filename: fname,
                };
              }
              return { kind: "workspace_file", path: ctx.path };
            }
            case "git_diff":
              return {
                kind: "git_diff",
                staged: ctx.staged,
                label: ctx.label,
                diff_text: ctx.staged ? stagedDiffText : workingDiffText,
              };
            case "terminal_output":
              return {
                kind: "terminal_output",
                label: ctx.label,
                content: ctx.content,
              };
            case "code_symbol":
              return {
                kind: "code_symbol",
                name: ctx.symbolName,
                symbol_kind: ctx.symbolKind,
                path: ctx.path,
                start_line: ctx.startLine,
                end_line: ctx.endLine,
                signature: ctx.signature ?? null,
                container: ctx.container ?? null,
                language: ctx.language,
                snippet: ctx.snippet ?? resolvedSymbolSnippets.get(ctx.id) ?? null,
              };
          }
        });

        composed = await orbitIpc.composePrompt({
          provider: providerForCompose,
          raw_text: rawDraft,
          explicit_directive: storedDirective,
          host_id: submitTarget.hostId,
          cwd,
          contexts: contextInputs,
          quotes: quotes.map((q) => ({
            path: q.path,
            line_number: q.lineNumber,
            code: q.code,
          })),
          pastes: pastes.map((p) => ({
            label: p.label,
            filename: p.filename ?? null,
            content: p.content,
          })),
        });
      } catch (err) {
        notifyToast(formatSubmitErrorToast(err));
        return;
      }

      const skillContexts = mergedContexts.filter(
        (c): c is Extract<ContextMentionItem, { kind: "skill" }> => c.kind === "skill",
      );
      const nonSkillContexts = mergedContexts.filter((c) => c.kind !== "skill");

      if (composed.is_fork) {
        if (submitTarget.isDraftNewSession || !submitTarget.agentId) {
          notifyToast(translateGlobal("composer.toastSessionRequiredFork"));
          return;
        }
        setState((s) => ({
          ...s,
          composerDrafts: { ...s.composerDrafts, [submitTarget.scopeKey]: "" },
          slashDirectives: { ...s.slashDirectives, [submitTarget.scopeKey]: null },
        }));
        draftPersistence.flushNow();
        void ctx.actions.forkAgent({
          hostId: submitTarget.hostId,
          sourceAgentId: submitTarget.agentId,
          worktree: Boolean(composed.fork_worktree),
          branch: composed.fork_branch ?? undefined,
          initialPrompt: composed.fork_prompt ?? undefined,
        });
        return;
      }

      if (composed.fast_mode_action) {
        setState((s) => ({
          ...s,
          composerDrafts: { ...s.composerDrafts, [submitTarget.scopeKey]: "" },
          slashDirectives: { ...s.slashDirectives, [submitTarget.scopeKey]: null },
        }));
        draftPersistence.flushNow();
        const stillOnOrigin =
          getState().selectedHostId === submitTarget.hostId &&
          (getState().isDraftNewSession || !getState().selectedAgentId) === submitTarget.isDraftNewSession &&
          (submitTarget.isDraftNewSession || getState().selectedAgentId === submitTarget.agentId);
        if (stillOnOrigin) {
          ctx.actions.toggleActiveFastMode(composed.fast_mode_action);
        }
        return;
      }

      if (composed.is_btw) {
        setState((s) => ({
          ...s,
          composerDrafts: { ...s.composerDrafts, [submitTarget.scopeKey]: "" },
        }));
        draftPersistence.flushNow();
        const stillOnOrigin =
          getState().selectedHostId === submitTarget.hostId &&
          (getState().isDraftNewSession || !getState().selectedAgentId) === submitTarget.isDraftNewSession &&
          (submitTarget.isDraftNewSession || getState().selectedAgentId === submitTarget.agentId);
        if (stillOnOrigin) {
          ctx.actions.openSideQuestionTab();
        }
        if (composed.btw_question) {
          void ctx.actions.submitSideQuestion(composed.btw_question, {
            hostId: submitTarget.hostId,
            scopeKey: submitTarget.scopeKey,
            isDraftNewSession: submitTarget.isDraftNewSession,
            agentId: submitTarget.agentId,
            workspaceCwd: submitTarget.workspaceCwd,
            draftProvider: submitTarget.draftProvider,
            draftAcpAgentId: submitTarget.draftAcpAgentId,
            draftModels: submitTarget.draftModels,
          });
        }
        return;
      }

      if (composed.is_compact) {
        if (submitTarget.isDraftNewSession || !submitTarget.agentId) {
          notifyToast(translateGlobal("composer.toastSessionRequiredCompact"));
          return;
        }
        const prevCompactDraft = getState().composerDrafts[submitTarget.scopeKey] ?? "";
        const prevCompactDirective = getState().slashDirectives[submitTarget.scopeKey] ?? null;
        setState((s) => ({
          ...s,
          composerDrafts: { ...s.composerDrafts, [submitTarget.scopeKey]: "" },
          slashDirectives: { ...s.slashDirectives, [submitTarget.scopeKey]: null },
        }));
        draftPersistence.flushNow();
        try {
          await orbitIpc.sendPrompt(submitTarget.hostId, submitTarget.agentId, composed.wire_prompt, {
            displayPrompt: composed.display_prompt,
            badges: composed.badges,
          });
        } catch (err) {
          setState((s) => ({
            ...s,
            composerDrafts: {
              ...s.composerDrafts,
              [submitTarget.scopeKey]: mergeRestoredDraftText(
                prevCompactDraft,
                s.composerDrafts[submitTarget.scopeKey] ?? "",
              ),
            },
            slashDirectives: {
              ...s.slashDirectives,
              [submitTarget.scopeKey]: s.slashDirectives[submitTarget.scopeKey] ?? prevCompactDirective,
            },
          }));
          draftPersistence.flushNow();
          notifyToast(formatSubmitErrorToast(err));
        }
        return;
      }

      if (composed.is_empty && skillContexts.length === 0) {
        return;
      }

      // Perform upfront validation BEFORE revoking preview URLs or clearing any draft state.
      const isCreatingNewAgent = submitTarget.isDraftNewSession || !submitTarget.agentId;
      const hostDetailForCreate = isCreatingNewAgent
        ? getState().hostDetails[submitTarget.hostId]
        : undefined;
      const installedProviders = hostDetailForCreate?.host.available_providers ?? [];
      const installedAcpList = (hostDetailForCreate?.acp_agents ?? []).filter(
        (a) => a.installed && a.spec.enabled,
      );
      if (
        isCreatingNewAgent &&
        hostDetailForCreate &&
        installedProviders.length === 0 &&
        installedAcpList.length === 0
      ) {
        ctx.actions.showToast(translateGlobal("composer.toastNoAgentCli"));
        return;
      }

      const effectiveDirective = composed.effective_directive;
      const cleanUserText = composed.display_prompt;
      const imagePathQueueByLabel = new Map<string, string[]>();
      const filePathQueueByLabel = new Map<string, string[]>();
      const sentImages: SentImagePreviewMeta[] = [];
      const targetAgentId = submitTarget.agentId;
      const pendingRevokeUrls = new Set<string>();
      for (const ctx of nonSkillContexts) {
        if (ctx.kind === "image") {
          if (ctx.path && ctx.previewUrl) {
            seedPromptImagePreview(submitTarget.hostId, ctx.path, ctx.previewUrl, true);
          } else if (ctx.previewUrl?.startsWith("blob:")) {
            pendingRevokeUrls.add(ctx.previewUrl);
          }
          const cachedEntry = ctx.path ? getCachedPromptImage(submitTarget.hostId, ctx.path) : null;
          const safePreviewUrl =
            ctx.previewUrl && !pendingRevokeUrls.has(ctx.previewUrl) ? ctx.previewUrl : null;
          const cachedPreview =
            safePreviewUrl || (cachedEntry?.status === "ready" ? cachedEntry.url : null);
          sentImages.push({
            label: ctx.filename,
            path: ctx.path || null,
            previewUrl: cachedPreview,
          });
          if (ctx.path) {
            const queue = imagePathQueueByLabel.get(ctx.filename);
            if (queue) queue.push(ctx.path);
            else imagePathQueueByLabel.set(ctx.filename, [ctx.path]);
          }
        } else if (ctx.kind === "file") {
          if (ctx.previewUrl?.startsWith("blob:")) {
            pendingRevokeUrls.add(ctx.previewUrl);
          }
          if (ctx.path) {
            const label = ctx.filename || basenameFromPath(ctx.path);
            const queue = filePathQueueByLabel.get(label);
            if (queue) queue.push(ctx.path);
            else filePathQueueByLabel.set(label, [ctx.path]);
          }
        }
      }
      const skillBadges: PromptBadge[] = skillContexts.map((sk) => ({
        kind: "skill",
        label: `/${sk.slug}`,
        path: null,
      }));
      const badges: PromptBadge[] = [
        ...composed.badges.map((b) => {
          if (b.path) return b;
          if (b.kind === "image") {
            const fallbackPath = imagePathQueueByLabel.get(b.label)?.shift();
            if (fallbackPath) return { ...b, path: fallbackPath };
          } else if (
            b.kind === "file" ||
            b.kind === "pdf" ||
            b.kind === "audio" ||
            b.kind === "video"
          ) {
            const fallbackPath = filePathQueueByLabel.get(b.label)?.shift();
            if (fallbackPath) return { ...b, path: fallbackPath };
          }
          return b;
        }),
        ...skillBadges,
      ];
      const skillPrefix =
        skillContexts.length > 0
          ? skillContexts
              .map((sk) => `Use skill /${sk.slug} (${sk.label}).`)
              .join("\n")
          : "";
      const assembledWirePrompt = skillPrefix
        ? composed.wire_prompt
          ? `${skillPrefix}\n\n${composed.wire_prompt}`
          : skillPrefix
        : composed.wire_prompt;

      // Snapshot all cleared per-scope slices BEFORE clearing them so any downstream failure
      // can restore the user's draft, mentions, attachments, pastes, and quotes without data loss.
      const draftSnapshot = captureComposerDraftSnapshot(submitTarget.scopeKey);

      setState(
        (s) => ({
          ...s,
          composerDrafts: { ...s.composerDrafts, [submitTarget.scopeKey]: "" },
          slashDirectives: { ...s.slashDirectives, [submitTarget.scopeKey]: null },
          inlineMentions: { ...s.inlineMentions, [submitTarget.scopeKey]: [] },
          contextItems: { ...s.contextItems, [submitTarget.scopeKey]: [] },
          pastedAttachments: { ...s.pastedAttachments, [submitTarget.scopeKey]: [] },
          diffQuotes: { ...s.diffQuotes, [submitTarget.scopeKey]: [] },
        }),
        true,
      );
      draftPersistence.flushNow();

      const commitSubmissionSuccess = () => {
        if (sentImages.length > 0) {
          recordRecentSentTurnImages(submitTarget.hostId, targetAgentId, cleanUserText, sentImages);
        }
        for (const url of pendingRevokeUrls) {
          URL.revokeObjectURL(url);
        }
        setState((s) => {
          const nextHistory =
            cleanUserText && s.promptHistory[s.promptHistory.length - 1] !== cleanUserText
              ? [...s.promptHistory.slice(-29), cleanUserText]
              : s.promptHistory;
          const shouldClearPreview =
            Boolean(s.mediaPreviewTarget?.localUrl) &&
            pendingRevokeUrls.has(s.mediaPreviewTarget!.localUrl!);
          if (nextHistory === s.promptHistory && !shouldClearPreview) {
            return s;
          }
          return {
            ...s,
            promptHistory: nextHistory,
            mediaPreviewTarget: shouldClearPreview ? null : s.mediaPreviewTarget,
          };
        }, true);
      };

      try {
        if (isCreatingNewAgent) {
          const effectiveProvider = resolveAvailableDraftProvider(
            hostDetailForCreate,
            submitTarget.draftProvider,
          );
          const effectiveAcpAgentId =
            effectiveProvider === "acp"
              ? submitTarget.draftAcpAgentId &&
                installedAcpList.some((a) => a.spec.id === submitTarget.draftAcpAgentId)
                ? submitTarget.draftAcpAgentId
                : (installedAcpList[0]?.spec.id ?? submitTarget.draftAcpAgentId)
              : null;

          const branchSeed =
            cleanUserText ||
            (effectiveDirective === "review"
              ? "code-review"
              : effectiveDirective === "plan"
                ? "plan"
                : badges[0]?.label ?? "task");

          const selectedCwd = submitTarget.workspaceCwd;
          const gitCacheKey = `${submitTarget.hostId}::${selectedCwd}`;
          const cachedInspection =
            getState().gitReposCache[gitCacheKey] ??
            hostDetailForCreate?.git_repos_by_cwd?.[selectedCwd] ??
            null;
          const sidebarProj = getState().sidebarView?.projects.find(
            (p) =>
              p.host_id === submitTarget.hostId &&
              matchesProjectDirectory(p.directories, selectedCwd),
          );
          const discoveredProj = hostDetailForCreate?.discovered_projects.find(
            (p) => p.path === selectedCwd,
          );
          const isDirectGitRepo =
            cachedInspection?.is_git_repo ??
            sidebarProj?.is_git_repo ??
            discoveredProj?.is_git_repo ??
            false;
          const nestedRepos =
            cachedInspection?.nested_git_repos ??
            sidebarProj?.nested_git_repos ??
            discoveredProj?.nested_git_repos ??
            [];
          const chosenNestedRepo = submitTarget.draftWorktreeRepoPath
            ? nestedRepos.find((r) => r.path === submitTarget.draftWorktreeRepoPath)?.path ??
              submitTarget.draftWorktreeRepoPath
            : null;

          let effectiveCwd = selectedCwd;
          let effectiveExtraDirs: string[] = [];
          let worktreeBranch: string | null = null;
          if (submitTarget.draftUseWorktree) {
            const canUseWorktree = isDirectGitRepo || Boolean(chosenNestedRepo);
            if (canUseWorktree) {
              const manual = submitTarget.draftWorktreeBranch.trim();
              if (manual) {
                worktreeBranch = manual;
              } else if (
                submitTarget.worktreeBranchPrefix.trim() &&
                submitTarget.worktreeBranchPrefix.trim() !== "auto"
              ) {
                worktreeBranch = slugifyPrefixedBranch(submitTarget.worktreeBranchPrefix, branchSeed);
              } else {
                worktreeBranch = "auto";
              }
              if (!isDirectGitRepo && chosenNestedRepo) {
                effectiveCwd = chosenNestedRepo;
                const parentDirs = new Set<string>([
                  selectedCwd,
                  ...(sidebarProj?.directories ?? []),
                ]);
                parentDirs.delete(chosenNestedRepo);
                parentDirs.delete("~");
                effectiveExtraDirs = Array.from(parentDirs).filter(Boolean);
              }
            }
          }
          const explicitDraftModel = submitTarget.draftModels[effectiveProvider] ?? null;
          const resolvedSel = await orbitIpc
            .resolveModelSelection({
              host_id: submitTarget.hostId,
              provider: effectiveProvider,
              desired_model: explicitDraftModel,
              desired_effort: submitTarget.draftEffort,
            })
            .catch(() => null);
          const cat = getCatalogForProvider(
            hostDetailForCreate?.provider_catalogs,
            effectiveProvider,
          );
          const spec = resolveActiveModelSpec(cat, explicitDraftModel);
          const resolvedModel =
            effectiveProvider === "acp"
              ? explicitDraftModel
              : (resolvedSel?.selected_model_id ?? explicitDraftModel ?? spec?.id ?? null);
          const resolvedEffort =
            effectiveProvider === "acp"
              ? null
              : (resolvedSel?.selected_effort ??
                resolveSupportedEffortForModel(
                  spec,
                  submitTarget.draftEffort,
                  cat.configured_default_effort,
                ));
          const supportsFast =
            effectiveProvider === "acp" || effectiveProvider === "antigravity"
              ? false
              : (resolvedSel?.supports_fast_mode ?? modelSupportsFastMode(effectiveProvider, spec));

          const stillOnOriginDraft =
            getState().selectedHostId === submitTarget.hostId &&
            getState().isDraftNewSession &&
            (getState().selectedWorkspaceCwd || "~") === submitTarget.workspaceCwd;
          let localPendingRef: PendingDraftCreateState | null = null;
          if (stillOnOriginDraft) {
            localPendingRef = {
              hostId: submitTarget.hostId,
              optimisticAgentId: null,
              startedAt: Date.now(),
            };
            ctx.setPendingDraftCreate(localPendingRef);
          }
          let optimisticId: string | undefined;
          try {
            optimisticId = await orbitIpc.createAgent({
              hostId: submitTarget.hostId,
              cwd: effectiveCwd,
              extraDirs: effectiveExtraDirs,
              provider: effectiveProvider,
              acpAgentId: effectiveAcpAgentId,
              model: resolvedModel,
              effort: resolvedEffort,
              fastMode: Boolean(submitTarget.draftFastMode && supportsFast),
              mode: submitTarget.draftMode,
              initialPrompt: assembledWirePrompt,
              initialDisplayPrompt: cleanUserText,
              initialBadges: badges,
              worktreeBranch,
              profileId: submitTarget.draftProfileId,
              workspaceId: submitTarget.draftWorkspaceId,
            });
          } catch (err) {
            if (localPendingRef && ctx.getPendingDraftCreate() === localPendingRef) {
              ctx.setPendingDraftCreate(null);
            }
            throw err;
          }
          commitSubmissionSuccess();
          const stillOnOriginDraftAfterCreate =
            getState().selectedHostId === submitTarget.hostId &&
            getState().isDraftNewSession &&
            (getState().selectedWorkspaceCwd || "~") === submitTarget.workspaceCwd;
          if (
            optimisticId &&
            stillOnOriginDraftAfterCreate &&
            localPendingRef &&
            ctx.getPendingDraftCreate() === localPendingRef
          ) {
            ctx.setPendingDraftCreate({
              hostId: submitTarget.hostId,
              optimisticAgentId: optimisticId,
              startedAt: Date.now(),
            });
          }
          if (stillOnOriginDraftAfterCreate) {
            setState((s) => ({
              ...s,
              draftProvider: effectiveProvider,
              draftWorktreeBranch: "",
              draftWorkspaceId: null,
            }));
          }
          return;
        }

        const agentId = submitTarget.agentId!;
        const detail = getState().hostDetails[submitTarget.hostId];
        const activeAgent = detail?.agents.find((a) => a.agent_id === agentId);
        const supportsServerQueue = Boolean(detail?.host.supports_server_queue);

        if (isAgentForegroundRunning(activeAgent) || activeAgent?.is_awaiting_permission) {
          if (steerImmediately) {
            await orbitIpc.sendPrompt(submitTarget.hostId, agentId, assembledWirePrompt, {
              displayPrompt: cleanUserText,
              badges,
              activeTurnBehavior: "steer",
            });
            commitSubmissionSuccess();
          } else if (supportsServerQueue) {
            const queuedContexts: ContextMentionItem[] = mergedContexts.map((ctx) => {
              if (ctx.kind === "file" && ctx.previewUrl && pendingRevokeUrls.has(ctx.previewUrl)) {
                return { ...ctx, previewUrl: undefined };
              }
              if (ctx.kind === "image" && ctx.previewUrl && pendingRevokeUrls.has(ctx.previewUrl)) {
                return { ...ctx, previewUrl: "" };
              }
              return ctx;
            });
            const queueId = `q_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
            rememberLocalQueuedDraftExtras(queueId, {
              slashDirective: effectiveDirective,
              contexts: queuedContexts,
              pastes,
              quotes,
            });
            try {
              await orbitIpc.enqueuePrompt(submitTarget.hostId, agentId, assembledWirePrompt, {
                displayPrompt: cleanUserText,
                badges,
                queueId,
              });
            } catch (err) {
              localQueuedDraftExtras.delete(queueId);
              throw err;
            }
            commitSubmissionSuccess();
          } else {
            const queuedContexts: ContextMentionItem[] = mergedContexts.map((ctx) => {
              if (ctx.kind === "file" && ctx.previewUrl && pendingRevokeUrls.has(ctx.previewUrl)) {
                return { ...ctx, previewUrl: undefined };
              }
              if (ctx.kind === "image" && ctx.previewUrl && pendingRevokeUrls.has(ctx.previewUrl)) {
                return { ...ctx, previewUrl: "" };
              }
              return ctx;
            });
            const queuedItem: QueuedPromptItem = {
              id: `q_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`,
              wirePrompt: assembledWirePrompt,
              displayText: cleanUserText,
              badges,
              slashDirective: effectiveDirective,
              contexts: queuedContexts,
              pastes,
              quotes,
            };
            setState((s) => ({
              ...s,
              queuedPrompts: {
                ...s.queuedPrompts,
                [submitTarget.scopeKey]: [...(s.queuedPrompts[submitTarget.scopeKey] ?? []), queuedItem],
              },
            }));
            commitSubmissionSuccess();
          }
          return;
        }

        setState((s) =>
          s.selectedHostId === submitTarget.hostId &&
          !s.isDraftNewSession &&
          s.selectedAgentId === agentId
            ? { ...s, sessionScrollNonce: s.sessionScrollNonce + 1 }
            : s,
        );
        await orbitIpc.sendPrompt(submitTarget.hostId, agentId, assembledWirePrompt, {
          displayPrompt: cleanUserText,
          badges,
        });
        commitSubmissionSuccess();
      } catch (err) {
        restoreComposerDraftSnapshot(submitTarget.scopeKey, draftSnapshot);
        notifyToast(formatSubmitErrorToast(err));
      }
    },


    removeQueuedPrompt(index: number, queueId?: string): void {
      const hostId = getState().selectedHostId;
      const agentId = getState().selectedAgentId;
      const detail = hostId ? getState().hostDetails[hostId] : undefined;
      if (hostId && agentId && detail?.host.supports_server_queue && queueId) {
        localQueuedDraftExtras.delete(queueId);
        void orbitIpc.removeQueuedPrompt(hostId, agentId, queueId);
        return;
      }

      const key = composerScopeKey(getState());
      setState((s) => ({
        ...s,
        queuedPrompts: {
          ...s.queuedPrompts,
          [key]: (s.queuedPrompts[key] ?? []).filter((_, idx) => idx !== index),
        },
      }));
    },


    popQueuedToDraft(index: number, queueId?: string): void {
      const key = composerScopeKey(getState());
      const hostId = getState().selectedHostId;
      const agentId = getState().selectedAgentId;
      const detail = hostId ? getState().hostDetails[hostId] : undefined;
      if (hostId && agentId && detail?.host.supports_server_queue && queueId) {
        const activeAgent = detail.agents.find((a) => a.agent_id === agentId);
        const serverTarget = activeAgent?.queued_prompts?.find((q) => q.queue_id === queueId);
        if (!serverTarget) return;
        const extras = localQueuedDraftExtras.get(queueId);
        localQueuedDraftExtras.delete(queueId);
        const restoredContexts: ContextMentionItem[] =
          extras?.contexts ??
          (serverTarget.badges ?? [])
            .filter((b) => b.kind === "file" && b.path)
            .map((b, bIdx) => ({
              id: `ctx_q_${Date.now()}_${bIdx}`,
              kind: "file" as const,
              path: b.path as string,
              filename: b.label,
            }));
        setState((s) => ({
          ...s,
          composerDrafts: {
            ...s.composerDrafts,
            [key]:
              s.composerDrafts[key] && serverTarget.display_text
                ? `${s.composerDrafts[key]}\n${serverTarget.display_text}`
                : serverTarget.display_text || s.composerDrafts[key] || "",
          },
          slashDirectives: {
            ...s.slashDirectives,
            [key]: extras?.slashDirective ?? s.slashDirectives[key] ?? null,
          },
          contextItems: {
            ...s.contextItems,
            [key]: [...(s.contextItems[key] ?? []), ...restoredContexts],
          },
          pastedAttachments: {
            ...s.pastedAttachments,
            [key]: [...(s.pastedAttachments[key] ?? []), ...(extras?.pastes ?? [])],
          },
          diffQuotes: {
            ...s.diffQuotes,
            [key]: [...(s.diffQuotes[key] ?? []), ...(extras?.quotes ?? [])],
          },
        }));
        void orbitIpc.removeQueuedPrompt(hostId, agentId, queueId);
        ctx.actions.refreshSidebarView();
        ctx.actions.focusComposer();
        return;
      }

      const list = getState().queuedPrompts[key] ?? [];
      const target = list[index];
      if (!target) return;
      setState((s) => ({
        ...s,
        composerDrafts: {
          ...s.composerDrafts,
          [key]:
            s.composerDrafts[key] && target.displayText
              ? `${s.composerDrafts[key]}\n${target.displayText}`
              : target.displayText || s.composerDrafts[key] || "",
        },
        slashDirectives: {
          ...s.slashDirectives,
          [key]: target.slashDirective ?? s.slashDirectives[key] ?? null,
        },
        contextItems: {
          ...s.contextItems,
          [key]: [...(s.contextItems[key] ?? []), ...target.contexts],
        },
        pastedAttachments: {
          ...s.pastedAttachments,
          [key]: [...(s.pastedAttachments[key] ?? []), ...target.pastes],
        },
        diffQuotes: {
          ...s.diffQuotes,
          [key]: [...(s.diffQuotes[key] ?? []), ...target.quotes],
        },
        queuedPrompts: {
          ...s.queuedPrompts,
          [key]: list.filter((_, idx) => idx !== index),
        },
      }));
      ctx.actions.refreshSidebarView();
      ctx.actions.focusComposer();
    },


    async flushQueuedPromptImmediately(index: number, queueId?: string): Promise<void> {
      const hostId = getState().selectedHostId;
      const agentId = getState().selectedAgentId;
      if (!hostId || !agentId) return;
      const detail = getState().hostDetails[hostId];
      if (detail?.host.supports_server_queue && queueId) {
        localQueuedDraftExtras.delete(queueId);
        setState((s) => ({ ...s, sessionScrollNonce: s.sessionScrollNonce + 1 }));
        await orbitIpc.flushQueuedPromptNow(hostId, agentId, queueId);
        return;
      }

      const key = `${hostId}::${agentId}`;
      const list = getState().queuedPrompts[key] ?? [];
      const target = list[index];
      if (!target) return;

      setState((s) => ({
        ...s,
        queuedPrompts: {
          ...s.queuedPrompts,
          [key]: list.filter((_, idx) => idx !== index),
        },
      }));
      const activeAgent = getState().hostDetails[hostId]?.agents.find((a) => a.agent_id === agentId);
      if (isAgentForegroundRunning(activeAgent) || activeAgent?.is_awaiting_permission) {
        await orbitIpc.interruptAgent(hostId, agentId);
      }
      await orbitIpc.sendPrompt(hostId, agentId, target.wirePrompt, {
        displayPrompt: target.displayText,
        badges: target.badges,
      });
    },


    reorderQueuedPrompts(fromIndex: number, toIndex: number): void {
      if (fromIndex === toIndex || fromIndex < 0 || toIndex < 0) return;
      const hostId = getState().selectedHostId;
      const agentId = getState().selectedAgentId;
      const detail = hostId ? getState().hostDetails[hostId] : undefined;
      if (hostId && agentId && detail?.host.supports_server_queue) {
        const activeAgent = detail.agents.find((a) => a.agent_id === agentId);
        const current = activeAgent?.queued_prompts ?? [];
        if (fromIndex >= current.length || toIndex >= current.length) return;
        const nextQueued = current.slice();
        const [moved] = nextQueued.splice(fromIndex, 1);
        if (!moved) return;
        nextQueued.splice(toIndex, 0, moved);
        const nextIds = nextQueued.map((q) => q.queue_id);
        const tlKey = `${hostId}::${agentId}`;
        setState((s) => {
          const curDetail = s.hostDetails[hostId];
          if (!curDetail) return s;
          const nextAgents = curDetail.agents.map((ag) =>
            ag.agent_id === agentId ? { ...ag, queued_prompts: nextQueued } : ag,
          );
          const curStage = s.stages[tlKey];
          const nextStages = curStage?.agent
            ? applyStageCacheUpdate(s, tlKey, {
                ...curStage,
                agent: { ...curStage.agent, queued_prompts: nextQueued },
              })
            : s.stages;
          return {
            ...s,
            hostDetails: {
              ...s.hostDetails,
              [hostId]: {
                ...curDetail,
                agents: nextAgents,
              },
            },
            stages: nextStages,
          };
        });
        void orbitIpc.reorderQueuedPrompts(hostId, agentId, nextIds);
        return;
      }

      const key = composerScopeKey(getState());
      const list = getState().queuedPrompts[key] ?? [];
      if (fromIndex >= list.length || toIndex >= list.length) return;
      const next = list.slice();
      const [moved] = next.splice(fromIndex, 1);
      if (!moved) return;
      next.splice(toIndex, 0, moved);
      setState((s) => ({
        ...s,
        queuedPrompts: {
          ...s.queuedPrompts,
          [key]: next,
        },
      }));
    },
  };
}
