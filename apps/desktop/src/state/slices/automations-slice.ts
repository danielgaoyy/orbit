import {
  type BrowserActionRequest,
  type BrowserActionResponseView,
  type BrowserPolicyConfig,
  type BrowserSessionSummary,
  type BrowserSessionsSnapshotView,
  type ContextOccupancyBreakdown,
  type GoalControlAction,
  type GoalRunRecord,
  type MonitorKind,
  type MonitorWatchRecord,
  type BestOfNCandidateSpec,
  type DagStepSpec,
  type WorkflowRunRecord,
  type MatchedPathRulesView,
  type PathRulesSnapshotView,
  type UpsertPathRuleParams,
  type ExternalMcpServerRecord,
  type EventTriggerDelivery,
  type EventTriggersSnapshotView,
  type UpsertEventTriggerRequest,
  type HookExecutionRecord,
  type HookScope,
  type LifecycleHookRule,
  type LifecycleHooksSnapshotView,
  type LspDiagnostic,
  type LspLocation,
  type LspServerStatus,
  type McpProbeResult,
  type GuardianConfigSnapshotView,
  type GuardianPolicyConfig,
  type PermissionRuleRecord,
  type UpsertPermissionRuleInput,
  type ImChannelTestResultView,
  type ImChannelsSnapshotView,
  type PushDevicesSnapshotView,
  type PushTestResultView,
  type UpdatePushPreferencesRequest,
  type UpsertImChannelRequest,
  type ProviderKind,
  type SessionMode,
  type UpsertExternalMcpServerRequest,
  type UsageInsightsSnapshot,
  orbitIpc,
} from "@/lib/orbit-ipc";
import {
  fileCacheKey,
  upsertLruRecord,
} from "../store-helpers";
import type { OrbitStoreContext } from "../store-runtime";

export function createAutomationsSlice(ctx: OrbitStoreContext) {
  const {
    dispatchDesktopNotification,
    getState,
    notifyToast,
    setState,
  } = ctx;

  const MAX_LSP_DIAGNOSTICS_CACHE_ENTRIES = 40;
  const MAX_LSP_SERVERS_CACHE_ENTRIES = 16;
  const MAX_LIFECYCLE_HOOKS_CACHE_ENTRIES = 16;
  let nextLspGotoSeq = 0;
  const lspDiagSeqByKey = new Map<string, number>();
  const lspServersSeqByKey = new Map<string, number>();
  const browserReqSeqByHost = new Map<string, number>();
  const lifecycleHooksSeqByKey = new Map<string, number>();
  const eventTriggersSeqByHost = new Map<string, number>();
  const guardianReqSeqByHost = new Map<string, number>();
  const permissionRulesReqSeqByHost = new Map<string, number>();
  const imChannelsReqSeqByHost = new Map<string, number>();
  const pushReqSeqByHost = new Map<string, number>();
  const pathRulesReqSeqByScope = new Map<string, number>();
  const externalMcpSnapshotSeqByHost: Record<string, number> = {};

  return {


    async loadGuardianConfig(hostId: string): Promise<GuardianConfigSnapshotView | null> {
      if (!hostId) return null;
      const seq = (guardianReqSeqByHost.get(hostId) ?? 0) + 1;
      guardianReqSeqByHost.set(hostId, seq);
      setState((s) => {
        const prev = s.guardianByHost[hostId];
        if (prev?.status === "ready") return s;
        return {
          ...s,
          guardianByHost: {
            ...s.guardianByHost,
            [hostId]: {
              status: "loading",
              config: prev?.config ?? null,
              recentAudits: prev?.recentAudits ?? [],
              saving: prev?.saving ?? false,
              error: null,
            },
          },
        };
      });
      try {
        const snap = await orbitIpc.getGuardianConfig(hostId);
        if (guardianReqSeqByHost.get(hostId) !== seq) return null;
        setState((s) => ({
          ...s,
          guardianByHost: {
            ...s.guardianByHost,
            [hostId]: {
              status: "ready",
              config: snap.config,
              recentAudits: snap.recent_audits,
              saving: false,
              error: null,
            },
          },
        }));
        return snap;
      } catch (err) {
        if (guardianReqSeqByHost.get(hostId) !== seq) return null;
        setState((s) => ({
          ...s,
          guardianByHost: {
            ...s.guardianByHost,
            [hostId]: {
              status: "error",
              config: s.guardianByHost[hostId]?.config ?? null,
              recentAudits: s.guardianByHost[hostId]?.recentAudits ?? [],
              saving: false,
              error: String(err),
            },
          },
        }));
        return null;
      }
    },


    async updateGuardianConfig(
      hostId: string,
      config: GuardianPolicyConfig,
    ): Promise<GuardianConfigSnapshotView | null> {
      if (!hostId) return null;
      const seq = (guardianReqSeqByHost.get(hostId) ?? 0) + 1;
      guardianReqSeqByHost.set(hostId, seq);
      const prevConfig = getState().guardianByHost[hostId]?.config ?? null;
      setState((s) => ({
        ...s,
        guardianByHost: {
          ...s.guardianByHost,
          [hostId]: {
            status: s.guardianByHost[hostId]?.status ?? "ready",
            config,
            recentAudits: s.guardianByHost[hostId]?.recentAudits ?? [],
            saving: true,
            error: null,
          },
        },
      }));
      try {
        const snap = await orbitIpc.updateGuardianConfig(hostId, config);
        if (guardianReqSeqByHost.get(hostId) !== seq) return null;
        setState((s) => ({
          ...s,
          guardianByHost: {
            ...s.guardianByHost,
            [hostId]: {
              status: "ready",
              config: snap.config,
              recentAudits: snap.recent_audits,
              saving: false,
              error: null,
            },
          },
        }));
        return snap;
      } catch (err) {
        if (guardianReqSeqByHost.get(hostId) !== seq) return null;
        setState((s) => ({
          ...s,
          guardianByHost: {
            ...s.guardianByHost,
            [hostId]: {
              status: "error",
              config: prevConfig,
              recentAudits: s.guardianByHost[hostId]?.recentAudits ?? [],
              saving: false,
              error: String(err),
            },
          },
        }));
        notifyToast(`Guardian update failed: ${String(err)}`);
        return null;
      }
    },

    async loadPermissionRules(
      hostId: string,
      projectCwd?: string | null,
    ): Promise<PermissionRuleRecord[] | null> {
      if (!hostId) return null;
      const seq = (permissionRulesReqSeqByHost.get(hostId) ?? 0) + 1;
      permissionRulesReqSeqByHost.set(hostId, seq);
      setState((s) => {
        const prev = s.permissionRulesByHost[hostId];
        if (prev?.status === "ready") return s;
        return {
          ...s,
          permissionRulesByHost: {
            ...s.permissionRulesByHost,
            [hostId]: {
              status: "loading",
              rules: prev?.rules ?? [],
              saving: prev?.saving ?? false,
              error: null,
            },
          },
        };
      });
      try {
        const rules = await orbitIpc.listPermissionRules(hostId, projectCwd ?? null);
        if (permissionRulesReqSeqByHost.get(hostId) !== seq) return null;
        setState((s) => ({
          ...s,
          permissionRulesByHost: {
            ...s.permissionRulesByHost,
            [hostId]: {
              status: "ready",
              rules,
              saving: false,
              error: null,
            },
          },
        }));
        return rules;
      } catch (err) {
        if (permissionRulesReqSeqByHost.get(hostId) !== seq) return null;
        setState((s) => ({
          ...s,
          permissionRulesByHost: {
            ...s.permissionRulesByHost,
            [hostId]: {
              status: "error",
              rules: s.permissionRulesByHost[hostId]?.rules ?? [],
              saving: false,
              error: String(err),
            },
          },
        }));
        return null;
      }
    },

    async upsertPermissionRule(
      hostId: string,
      rule: UpsertPermissionRuleInput,
    ): Promise<PermissionRuleRecord | null> {
      if (!hostId) return null;
      setState((s) => ({
        ...s,
        permissionRulesByHost: {
          ...s.permissionRulesByHost,
          [hostId]: {
            status: s.permissionRulesByHost[hostId]?.status ?? "ready",
            rules: s.permissionRulesByHost[hostId]?.rules ?? [],
            saving: true,
            error: null,
          },
        },
      }));
      try {
        const saved = await orbitIpc.upsertPermissionRule(hostId, rule);
        setState((s) => {
          const prev = s.permissionRulesByHost[hostId]?.rules ?? [];
          const exists = prev.some((r) => r.id === saved.id);
          const nextRules = exists
            ? prev.map((r) => (r.id === saved.id ? saved : r))
            : [saved, ...prev];
          return {
            ...s,
            permissionRulesByHost: {
              ...s.permissionRulesByHost,
              [hostId]: {
                status: "ready",
                rules: nextRules,
                saving: false,
                error: null,
              },
            },
          };
        });
        return saved;
      } catch (err) {
        setState((s) => ({
          ...s,
          permissionRulesByHost: {
            ...s.permissionRulesByHost,
            [hostId]: {
              status: s.permissionRulesByHost[hostId]?.status ?? "ready",
              rules: s.permissionRulesByHost[hostId]?.rules ?? [],
              saving: false,
              error: String(err),
            },
          },
        }));
        notifyToast(String(err));
        return null;
      }
    },

    async deletePermissionRule(hostId: string, ruleId: string): Promise<boolean> {
      if (!hostId || !ruleId) return false;
      setState((s) => ({
        ...s,
        permissionRulesByHost: {
          ...s.permissionRulesByHost,
          [hostId]: {
            status: s.permissionRulesByHost[hostId]?.status ?? "ready",
            rules: s.permissionRulesByHost[hostId]?.rules ?? [],
            saving: true,
            error: null,
          },
        },
      }));
      try {
        const deleted = await orbitIpc.deletePermissionRule(hostId, ruleId);
        setState((s) => {
          const prev = s.permissionRulesByHost[hostId]?.rules ?? [];
          return {
            ...s,
            permissionRulesByHost: {
              ...s.permissionRulesByHost,
              [hostId]: {
                status: "ready",
                rules: prev.filter((r) => r.id !== ruleId),
                saving: false,
                error: null,
              },
            },
          };
        });
        return deleted;
      } catch (err) {
        setState((s) => ({
          ...s,
          permissionRulesByHost: {
            ...s.permissionRulesByHost,
            [hostId]: {
              status: s.permissionRulesByHost[hostId]?.status ?? "ready",
              rules: s.permissionRulesByHost[hostId]?.rules ?? [],
              saving: false,
              error: String(err),
            },
          },
        }));
        notifyToast(String(err));
        return false;
      }
    },


    async loadImChannels(hostId: string): Promise<ImChannelsSnapshotView | null> {
      if (!hostId) return null;
      const seq = (imChannelsReqSeqByHost.get(hostId) ?? 0) + 1;
      imChannelsReqSeqByHost.set(hostId, seq);
      setState((s) => {
        const prev = s.imChannelsByHost[hostId];
        if (prev?.status === "ready") return s;
        return {
          ...s,
          imChannelsByHost: {
            ...s.imChannelsByHost,
            [hostId]: {
              status: "loading",
              channels: prev?.channels ?? [],
              bindings: prev?.bindings ?? [],
              baseWebhookUrl: prev?.baseWebhookUrl ?? "",
              saving: prev?.saving ?? false,
              error: null,
            },
          },
        };
      });
      try {
        const snap = await orbitIpc.listImChannels(hostId);
        if (imChannelsReqSeqByHost.get(hostId) !== seq) return null;
        setState((s) => ({
          ...s,
          imChannelsByHost: {
            ...s.imChannelsByHost,
            [hostId]: {
              status: "ready",
              channels: snap.channels,
              bindings: snap.bindings,
              baseWebhookUrl: snap.base_webhook_url,
              saving: false,
              error: null,
            },
          },
        }));
        return snap;
      } catch (err) {
        if (imChannelsReqSeqByHost.get(hostId) !== seq) return null;
        setState((s) => ({
          ...s,
          imChannelsByHost: {
            ...s.imChannelsByHost,
            [hostId]: {
              status: "error",
              channels: s.imChannelsByHost[hostId]?.channels ?? [],
              bindings: s.imChannelsByHost[hostId]?.bindings ?? [],
              baseWebhookUrl: s.imChannelsByHost[hostId]?.baseWebhookUrl ?? "",
              saving: false,
              error: String(err),
            },
          },
        }));
        return null;
      }
    },


    async upsertImChannel(
      hostId: string,
      request: UpsertImChannelRequest,
    ): Promise<ImChannelsSnapshotView | null> {
      if (!hostId) return null;
      const seq = (imChannelsReqSeqByHost.get(hostId) ?? 0) + 1;
      imChannelsReqSeqByHost.set(hostId, seq);
      setState((s) => ({
        ...s,
        imChannelsByHost: {
          ...s.imChannelsByHost,
          [hostId]: {
            status: s.imChannelsByHost[hostId]?.status ?? "ready",
            channels: s.imChannelsByHost[hostId]?.channels ?? [],
            bindings: s.imChannelsByHost[hostId]?.bindings ?? [],
            baseWebhookUrl: s.imChannelsByHost[hostId]?.baseWebhookUrl ?? "",
            saving: true,
            error: null,
          },
        },
      }));
      try {
        const snap = await orbitIpc.upsertImChannel(hostId, request);
        if (imChannelsReqSeqByHost.get(hostId) !== seq) return null;
        setState((s) => ({
          ...s,
          imChannelsByHost: {
            ...s.imChannelsByHost,
            [hostId]: {
              status: "ready",
              channels: snap.channels,
              bindings: snap.bindings,
              baseWebhookUrl: snap.base_webhook_url,
              saving: false,
              error: null,
            },
          },
        }));
        return snap;
      } catch (err) {
        if (imChannelsReqSeqByHost.get(hostId) !== seq) return null;
        setState((s) => ({
          ...s,
          imChannelsByHost: {
            ...s.imChannelsByHost,
            [hostId]: {
              status: s.imChannelsByHost[hostId]?.status ?? "ready",
              channels: s.imChannelsByHost[hostId]?.channels ?? [],
              bindings: s.imChannelsByHost[hostId]?.bindings ?? [],
              baseWebhookUrl: s.imChannelsByHost[hostId]?.baseWebhookUrl ?? "",
              saving: false,
              error: String(err),
            },
          },
        }));
        notifyToast(`IM channel save failed: ${String(err)}`);
        return null;
      }
    },


    async deleteImChannel(
      hostId: string,
      channelId: string,
    ): Promise<ImChannelsSnapshotView | null> {
      if (!hostId || !channelId) return null;
      const seq = (imChannelsReqSeqByHost.get(hostId) ?? 0) + 1;
      imChannelsReqSeqByHost.set(hostId, seq);
      try {
        const snap = await orbitIpc.deleteImChannel(hostId, channelId);
        if (imChannelsReqSeqByHost.get(hostId) !== seq) return null;
        setState((s) => ({
          ...s,
          imChannelsByHost: {
            ...s.imChannelsByHost,
            [hostId]: {
              status: "ready",
              channels: snap.channels,
              bindings: snap.bindings,
              baseWebhookUrl: snap.base_webhook_url,
              saving: false,
              error: null,
            },
          },
        }));
        return snap;
      } catch (err) {
        notifyToast(`Delete IM channel failed: ${String(err)}`);
        return null;
      }
    },


    async testImChannel(
      hostId: string,
      channelId: string,
      targetChatId: string,
      message: string,
    ): Promise<ImChannelTestResultView> {
      try {
        const res = await orbitIpc.testImChannel(hostId, channelId, targetChatId, message);
        void ctx.actions.loadImChannels(hostId);
        return res;
      } catch (err) {
        return { ok: false, detail: String(err) };
      }
    },


    async loadPushDevices(hostId: string): Promise<PushDevicesSnapshotView | null> {
      if (!hostId) return null;
      const seq = (pushReqSeqByHost.get(hostId) ?? 0) + 1;
      pushReqSeqByHost.set(hostId, seq);
      setState((s) => ({
        ...s,
        pushByHost: {
          ...s.pushByHost,
          [hostId]: {
            status: "loading",
            devices: s.pushByHost[hostId]?.devices ?? [],
            preferences: s.pushByHost[hostId]?.preferences ?? null,
            saving: s.pushByHost[hostId]?.saving ?? false,
            testing: s.pushByHost[hostId]?.testing ?? false,
            error: null,
          },
        },
      }));
      try {
        const snap = await orbitIpc.listPushDevices(hostId);
        if (pushReqSeqByHost.get(hostId) !== seq) return null;
        setState((s) => ({
          ...s,
          pushByHost: {
            ...s.pushByHost,
            [hostId]: {
              status: "ready",
              devices: snap.devices,
              preferences: snap.preferences,
              saving: false,
              testing: false,
              error: null,
            },
          },
        }));
        return snap;
      } catch (err) {
        if (pushReqSeqByHost.get(hostId) !== seq) return null;
        setState((s) => ({
          ...s,
          pushByHost: {
            ...s.pushByHost,
            [hostId]: {
              status: "error",
              devices: s.pushByHost[hostId]?.devices ?? [],
              preferences: s.pushByHost[hostId]?.preferences ?? null,
              saving: false,
              testing: false,
              error: String(err),
            },
          },
        }));
        return null;
      }
    },


    async updatePushPreferences(
      hostId: string,
      request: UpdatePushPreferencesRequest,
    ): Promise<PushDevicesSnapshotView | null> {
      if (!hostId) return null;
      const seq = (pushReqSeqByHost.get(hostId) ?? 0) + 1;
      pushReqSeqByHost.set(hostId, seq);
      setState((s) => ({
        ...s,
        pushByHost: {
          ...s.pushByHost,
          [hostId]: {
            status: s.pushByHost[hostId]?.status ?? "ready",
            devices: s.pushByHost[hostId]?.devices ?? [],
            preferences: s.pushByHost[hostId]?.preferences ?? null,
            saving: true,
            testing: s.pushByHost[hostId]?.testing ?? false,
            error: null,
          },
        },
      }));
      try {
        const snap = await orbitIpc.updatePushPreferences(hostId, request);
        if (pushReqSeqByHost.get(hostId) !== seq) return null;
        setState((s) => ({
          ...s,
          pushByHost: {
            ...s.pushByHost,
            [hostId]: {
              status: "ready",
              devices: snap.devices,
              preferences: snap.preferences,
              saving: false,
              testing: false,
              error: null,
            },
          },
        }));
        notifyToast("Push settings saved");
        return snap;
      } catch (err) {
        if (pushReqSeqByHost.get(hostId) !== seq) return null;
        setState((s) => ({
          ...s,
          pushByHost: {
            ...s.pushByHost,
            [hostId]: {
              status: s.pushByHost[hostId]?.status ?? "ready",
              devices: s.pushByHost[hostId]?.devices ?? [],
              preferences: s.pushByHost[hostId]?.preferences ?? null,
              saving: false,
              testing: false,
              error: String(err),
            },
          },
        }));
        notifyToast(`Save push settings failed: ${String(err)}`);
        return null;
      }
    },


    async unregisterPushDevice(
      hostId: string,
      deviceId: string,
    ): Promise<PushDevicesSnapshotView | null> {
      if (!hostId || !deviceId) return null;
      const seq = (pushReqSeqByHost.get(hostId) ?? 0) + 1;
      pushReqSeqByHost.set(hostId, seq);
      try {
        const snap = await orbitIpc.unregisterPushDevice(hostId, deviceId);
        if (pushReqSeqByHost.get(hostId) !== seq) return null;
        setState((s) => ({
          ...s,
          pushByHost: {
            ...s.pushByHost,
            [hostId]: {
              status: "ready",
              devices: snap.devices,
              preferences: snap.preferences,
              saving: false,
              testing: false,
              error: null,
            },
          },
        }));
        notifyToast("Device removed");
        return snap;
      } catch (err) {
        notifyToast(`Remove device failed: ${String(err)}`);
        return null;
      }
    },


    async testPushNotification(
      hostId: string,
      deviceId?: string | null,
    ): Promise<PushTestResultView> {
      if (!hostId) {
        return { attempted: 0, delivered: 0, detail: "No host selected." };
      }
      setState((s) => ({
        ...s,
        pushByHost: {
          ...s.pushByHost,
          [hostId]: {
            status: s.pushByHost[hostId]?.status ?? "ready",
            devices: s.pushByHost[hostId]?.devices ?? [],
            preferences: s.pushByHost[hostId]?.preferences ?? null,
            saving: s.pushByHost[hostId]?.saving ?? false,
            testing: true,
            error: null,
          },
        },
      }));
      try {
        const res = await orbitIpc.testPushNotification(hostId, deviceId ?? null);
        setState((s) => ({
          ...s,
          pushByHost: {
            ...s.pushByHost,
            [hostId]: {
              status: s.pushByHost[hostId]?.status ?? "ready",
              devices: s.pushByHost[hostId]?.devices ?? [],
              preferences: s.pushByHost[hostId]?.preferences ?? null,
              saving: s.pushByHost[hostId]?.saving ?? false,
              testing: false,
              error: s.pushByHost[hostId]?.error ?? null,
            },
          },
        }));
        notifyToast(res.detail);
        void ctx.actions.loadPushDevices(hostId);
        return res;
      } catch (err) {
        const detail = String(err);
        setState((s) => ({
          ...s,
          pushByHost: {
            ...s.pushByHost,
            [hostId]: {
              status: s.pushByHost[hostId]?.status ?? "ready",
              devices: s.pushByHost[hostId]?.devices ?? [],
              preferences: s.pushByHost[hostId]?.preferences ?? null,
              saving: false,
              testing: false,
              error: detail,
            },
          },
        }));
        notifyToast(`Test push failed: ${detail}`);
        return { attempted: 0, delivered: 0, detail };
      }
    },


    sendTestDesktopNotification(): boolean {
      const hostId = getState().selectedHostId ?? getState().fleet.hosts[0]?.host_id ?? "";
      const hostName =
        getState().fleet.hosts.find((h) => h.host_id === hostId)?.display_name ?? "Orbit";
      const ok = dispatchDesktopNotification({
        title: `Orbit · ${hostName}`,
        body: "Click to focus Orbit and jump to your active session.",
        hostId,
        agentId: getState().selectedAgentId,
        tag: "orbit-desktop-test",
        forceEvenIfVisible: true,
      });
      notifyToast(ok ? "Desktop test notification sent" : "Enable desktop notification permission first");
      return ok;
    },


    async loadLspServers(hostId: string, cwd: string): Promise<LspServerStatus[]> {
      if (!hostId || !cwd || cwd === "~") return [];
      const scopeKey = `${hostId}::${cwd}`;
      const seq = (lspServersSeqByKey.get(scopeKey) ?? 0) + 1;
      lspServersSeqByKey.set(scopeKey, seq);
      setState((s) => {
        const prev = s.lspServersByWorkspace[scopeKey];
        if (prev?.status === "ready") return s;
        return {
          ...s,
          lspServersByWorkspace: upsertLruRecord(
            s.lspServersByWorkspace,
            scopeKey,
            {
              status: "loading",
              servers: prev?.servers ?? [],
              error: null,
            },
            MAX_LSP_SERVERS_CACHE_ENTRIES,
          ),
        };
      });
      try {
        const servers = await orbitIpc.listLspServers(hostId, cwd);
        if (lspServersSeqByKey.get(scopeKey) !== seq) return [];
        setState((s) => ({
          ...s,
          lspServersByWorkspace: upsertLruRecord(
            s.lspServersByWorkspace,
            scopeKey,
            { status: "ready", servers, error: null },
            MAX_LSP_SERVERS_CACHE_ENTRIES,
          ),
        }));
        return servers;
      } catch (err) {
        if (lspServersSeqByKey.get(scopeKey) !== seq) return [];
        setState((s) => ({
          ...s,
          lspServersByWorkspace: upsertLruRecord(
            s.lspServersByWorkspace,
            scopeKey,
            {
              status: "error",
              servers: s.lspServersByWorkspace[scopeKey]?.servers ?? [],
              error: String(err),
            },
            MAX_LSP_SERVERS_CACHE_ENTRIES,
          ),
        }));
        return [];
      }
    },


    async refreshConditionalTimers(
      hostId: string,
      agentId?: string | null,
      includeResolved = true,
    ): Promise<import("@/lib/orbit-ipc").ConditionalTimerRecord[]> {
      if (!hostId) return [];
      const timers = await orbitIpc.listConditionalTimers(hostId, agentId, includeResolved);
      if (!agentId && includeResolved) {
        setState((s) => {
          const detail = s.hostDetails[hostId];
          if (!detail) return s;
          return {
            ...s,
            hostDetails: {
              ...s.hostDetails,
              [hostId]: { ...detail, conditional_timers: timers },
            },
          };
        });
      }
      return timers;
    },


    async createConditionalTimer(params: {
      hostId: string;
      agentId: string;
      durationSeconds: number;
      prompt: string;
      condition: import("@/lib/orbit-ipc").TimerCondition;
    }): Promise<import("@/lib/orbit-ipc").ConditionalTimerRecord> {
      const created = await orbitIpc.createConditionalTimer(params);
      setState((s) => {
        const detail = s.hostDetails[params.hostId];
        if (!detail) return s;
        const existing = detail.conditional_timers ?? [];
        const next = [created, ...existing.filter((t) => t.timer_id !== created.timer_id)].sort(
          (a, b) => {
            const aArmed = a.state === "armed";
            const bArmed = b.state === "armed";
            if (aArmed !== bArmed) return aArmed ? -1 : 1;
            if (aArmed) return a.fire_at_ms - b.fire_at_ms;
            return (b.resolved_at_ms ?? b.created_at_ms) - (a.resolved_at_ms ?? a.created_at_ms);
          },
        );
        return {
          ...s,
          hostDetails: {
            ...s.hostDetails,
            [params.hostId]: { ...detail, conditional_timers: next },
          },
        };
      });
      return created;
    },


    async cancelConditionalTimer(
      hostId: string,
      timerId: string,
    ): Promise<import("@/lib/orbit-ipc").ConditionalTimerRecord> {
      const updated = await orbitIpc.cancelConditionalTimer(hostId, timerId);
      setState((s) => {
        const detail = s.hostDetails[hostId];
        if (!detail) return s;
        const existing = detail.conditional_timers ?? [];
        const next = [updated, ...existing.filter((t) => t.timer_id !== updated.timer_id)].sort(
          (a, b) => {
            const aArmed = a.state === "armed";
            const bArmed = b.state === "armed";
            if (aArmed !== bArmed) return aArmed ? -1 : 1;
            if (aArmed) return a.fire_at_ms - b.fire_at_ms;
            return (b.resolved_at_ms ?? b.created_at_ms) - (a.resolved_at_ms ?? a.created_at_ms);
          },
        );
        return {
          ...s,
          hostDetails: {
            ...s.hostDetails,
            [hostId]: { ...detail, conditional_timers: next },
          },
        };
      });
      return updated;
    },


    async fireConditionalTimerNow(
      hostId: string,
      timerId: string,
    ): Promise<import("@/lib/orbit-ipc").ConditionalTimerRecord> {
      const updated = await orbitIpc.fireConditionalTimerNow(hostId, timerId);
      setState((s) => {
        const detail = s.hostDetails[hostId];
        if (!detail) return s;
        const existing = detail.conditional_timers ?? [];
        const next = [updated, ...existing.filter((t) => t.timer_id !== updated.timer_id)].sort(
          (a, b) => {
            const aArmed = a.state === "armed";
            const bArmed = b.state === "armed";
            if (aArmed !== bArmed) return aArmed ? -1 : 1;
            if (aArmed) return a.fire_at_ms - b.fire_at_ms;
            return (b.resolved_at_ms ?? b.created_at_ms) - (a.resolved_at_ms ?? a.created_at_ms);
          },
        );
        return {
          ...s,
          hostDetails: {
            ...s.hostDetails,
            [hostId]: { ...detail, conditional_timers: next },
          },
        };
      });
      return updated;
    },


    async restartLspServer(hostId: string, cwd: string, serverId: string): Promise<void> {
      if (!hostId || !cwd || cwd === "~" || !serverId) return;
      const scopeKey = `${hostId}::${cwd}`;
      const seq = (lspServersSeqByKey.get(scopeKey) ?? 0) + 1;
      lspServersSeqByKey.set(scopeKey, seq);
      try {
        const servers = await orbitIpc.restartLspServer(hostId, cwd, serverId);
        if (lspServersSeqByKey.get(scopeKey) !== seq) return;
        setState((s) => ({
          ...s,
          lspServersByWorkspace: upsertLruRecord(
            s.lspServersByWorkspace,
            scopeKey,
            { status: "ready", servers, error: null },
            MAX_LSP_SERVERS_CACHE_ENTRIES,
          ),
        }));
        notifyToast(`Restarted ${serverId}`);
      } catch (err) {
        if (lspServersSeqByKey.get(scopeKey) !== seq) return;
        notifyToast(`Restart failed: ${String(err)}`);
      }
    },


    async getUsageInsights(hostId: string, days = 7): Promise<UsageInsightsSnapshot> {
      return orbitIpc.getUsageInsights(hostId, days);
    },


    async getContextOccupancy(
      hostId: string,
      agentId: string,
    ): Promise<ContextOccupancyBreakdown> {
      return orbitIpc.getContextOccupancy(hostId, agentId);
    },


    async loadLifecycleHooks(
      hostId: string,
      cwd: string | null,
    ): Promise<LifecycleHooksSnapshotView | null> {
      if (!hostId) return null;
      const normalizedCwd = cwd && cwd.trim() && cwd.trim() !== "~" ? cwd.trim() : null;
      const scopeKey = `${hostId}::${normalizedCwd ?? "__global__"}`;
      const seq = (lifecycleHooksSeqByKey.get(scopeKey) ?? 0) + 1;
      lifecycleHooksSeqByKey.set(scopeKey, seq);
      setState((s) => {
        const prev = s.lifecycleHooksByScope[scopeKey];
        if (prev?.status === "ready") return s;
        return {
          ...s,
          lifecycleHooksByScope: upsertLruRecord(
            s.lifecycleHooksByScope,
            scopeKey,
            {
              status: "loading",
              hooks: prev?.hooks ?? [],
              recentExecutions: prev?.recentExecutions ?? [],
              projectHooksTrusted: prev?.projectHooksTrusted ?? false,
              error: null,
            },
            MAX_LIFECYCLE_HOOKS_CACHE_ENTRIES,
          ),
        };
      });
      try {
        const snap = await orbitIpc.listLifecycleHooks(hostId, normalizedCwd);
        if (lifecycleHooksSeqByKey.get(scopeKey) !== seq) return null;
        setState((s) => ({
          ...s,
          lifecycleHooksByScope: upsertLruRecord(
            s.lifecycleHooksByScope,
            scopeKey,
            {
              status: "ready",
              hooks: snap.hooks,
              recentExecutions: snap.recent_executions,
              projectHooksTrusted: snap.project_hooks_trusted,
              error: null,
            },
            MAX_LIFECYCLE_HOOKS_CACHE_ENTRIES,
          ),
        }));
        return snap;
      } catch (err) {
        if (lifecycleHooksSeqByKey.get(scopeKey) !== seq) return null;
        setState((s) => {
          const prev = s.lifecycleHooksByScope[scopeKey];
          return {
            ...s,
            lifecycleHooksByScope: upsertLruRecord(
              s.lifecycleHooksByScope,
              scopeKey,
              {
                status: "error",
                hooks: prev?.hooks ?? [],
                recentExecutions: prev?.recentExecutions ?? [],
                projectHooksTrusted: prev?.projectHooksTrusted ?? false,
                error: String(err),
              },
              MAX_LIFECYCLE_HOOKS_CACHE_ENTRIES,
            ),
          };
        });
        return null;
      }
    },


    async upsertLifecycleHook(
      hostId: string,
      cwd: string | null,
      hook: LifecycleHookRule,
    ): Promise<LifecycleHooksSnapshotView> {
      const normalizedCwd = cwd && cwd.trim() && cwd.trim() !== "~" ? cwd.trim() : null;
      const scopeKey = `${hostId}::${normalizedCwd ?? "__global__"}`;
      const seq = (lifecycleHooksSeqByKey.get(scopeKey) ?? 0) + 1;
      lifecycleHooksSeqByKey.set(scopeKey, seq);
      try {
        const snap = await orbitIpc.upsertLifecycleHook(hostId, normalizedCwd, hook);
        if (lifecycleHooksSeqByKey.get(scopeKey) === seq) {
          setState((s) => ({
            ...s,
            lifecycleHooksByScope: upsertLruRecord(
              s.lifecycleHooksByScope,
              scopeKey,
              {
                status: "ready",
                hooks: snap.hooks,
                recentExecutions: snap.recent_executions,
                projectHooksTrusted: snap.project_hooks_trusted,
                error: null,
              },
              MAX_LIFECYCLE_HOOKS_CACHE_ENTRIES,
            ),
          }));
        }
        return snap;
      } catch (err) {
        notifyToast(`Hook update failed: ${String(err)}`);
        throw err;
      }
    },


    async deleteLifecycleHook(
      hostId: string,
      cwd: string | null,
      hookId: string,
      scope: HookScope,
    ): Promise<LifecycleHooksSnapshotView> {
      const normalizedCwd = cwd && cwd.trim() && cwd.trim() !== "~" ? cwd.trim() : null;
      const scopeKey = `${hostId}::${normalizedCwd ?? "__global__"}`;
      const seq = (lifecycleHooksSeqByKey.get(scopeKey) ?? 0) + 1;
      lifecycleHooksSeqByKey.set(scopeKey, seq);
      const snap = await orbitIpc.deleteLifecycleHook(hostId, normalizedCwd, hookId, scope);
      if (lifecycleHooksSeqByKey.get(scopeKey) === seq) {
        setState((s) => ({
          ...s,
          lifecycleHooksByScope: upsertLruRecord(
            s.lifecycleHooksByScope,
            scopeKey,
            {
              status: "ready",
              hooks: snap.hooks,
              recentExecutions: snap.recent_executions,
              projectHooksTrusted: snap.project_hooks_trusted,
              error: null,
            },
            MAX_LIFECYCLE_HOOKS_CACHE_ENTRIES,
          ),
        }));
      }
      notifyToast("Hook deleted");
      return snap;
    },


    async setProjectHooksTrust(
      hostId: string,
      cwd: string,
      trusted: boolean,
    ): Promise<LifecycleHooksSnapshotView> {
      const scopeKey = `${hostId}::${cwd}`;
      const seq = (lifecycleHooksSeqByKey.get(scopeKey) ?? 0) + 1;
      lifecycleHooksSeqByKey.set(scopeKey, seq);
      try {
        const snap = await orbitIpc.setProjectHooksTrust(hostId, cwd, trusted);
        if (lifecycleHooksSeqByKey.get(scopeKey) === seq) {
          setState((s) => ({
            ...s,
            lifecycleHooksByScope: upsertLruRecord(
              s.lifecycleHooksByScope,
              scopeKey,
              {
                status: "ready",
                hooks: snap.hooks,
                recentExecutions: snap.recent_executions,
                projectHooksTrusted: snap.project_hooks_trusted,
                error: null,
              },
              MAX_LIFECYCLE_HOOKS_CACHE_ENTRIES,
            ),
          }));
        }
        notifyToast(trusted ? "Project hooks trusted" : "Project hook trust revoked");
        return snap;
      } catch (err) {
        notifyToast(`Trust update failed: ${String(err)}`);
        throw err;
      }
    },


    async testLifecycleHook(
      hostId: string,
      cwd: string | null,
      hookId: string,
      samplePayload: Record<string, unknown>,
    ): Promise<HookExecutionRecord> {
      const normalizedCwd = cwd && cwd.trim() && cwd.trim() !== "~" ? cwd.trim() : null;
      const scopeKey = `${hostId}::${normalizedCwd ?? "__global__"}`;
      try {
        const record = await orbitIpc.testLifecycleHook(
          hostId,
          normalizedCwd,
          hookId,
          samplePayload,
        );
        setState((s) => {
          const prev = s.lifecycleHooksByScope[scopeKey];
          if (!prev) return s;
          const nextRuns = [
            record,
            ...prev.recentExecutions.filter((r) => r.id !== record.id),
          ].slice(0, 100);
          return {
            ...s,
            lifecycleHooksByScope: upsertLruRecord(
              s.lifecycleHooksByScope,
              scopeKey,
              {
                ...prev,
                recentExecutions: nextRuns,
              },
              MAX_LIFECYCLE_HOOKS_CACHE_ENTRIES,
            ),
          };
        });
        notifyToast(
          `${record.hook_name}: ${record.decision.toUpperCase()} (${record.duration_ms}ms)`,
        );
        return record;
      } catch (err) {
        notifyToast(`Hook test failed: ${String(err)}`);
        throw err;
      }
    },


    async loadEventTriggers(hostId: string): Promise<EventTriggersSnapshotView | null> {
      if (!hostId) return null;
      const seq = (eventTriggersSeqByHost.get(hostId) ?? 0) + 1;
      eventTriggersSeqByHost.set(hostId, seq);
      setState((s) => {
        const prev = s.eventTriggersByHost[hostId];
        return {
          ...s,
          eventTriggersByHost: {
            ...s.eventTriggersByHost,
            [hostId]: {
              status: "loading",
              triggers: prev?.triggers ?? [],
              recentDeliveries: prev?.recentDeliveries ?? [],
              baseWebhookUrl: prev?.baseWebhookUrl ?? "http://127.0.0.1:6868",
              error: null,
            },
          },
        };
      });
      try {
        const snap = await orbitIpc.listEventTriggers(hostId);
        if (eventTriggersSeqByHost.get(hostId) !== seq) return null;
        setState((s) => ({
          ...s,
          eventTriggersByHost: {
            ...s.eventTriggersByHost,
            [hostId]: {
              status: "ready",
              triggers: snap.triggers,
              recentDeliveries: snap.recent_deliveries,
              baseWebhookUrl: snap.base_webhook_url,
              error: null,
            },
          },
        }));
        return snap;
      } catch (err) {
        if (eventTriggersSeqByHost.get(hostId) !== seq) return null;
        setState((s) => {
          const prev = s.eventTriggersByHost[hostId];
          return {
            ...s,
            eventTriggersByHost: {
              ...s.eventTriggersByHost,
              [hostId]: {
                status: "error",
                triggers: prev?.triggers ?? [],
                recentDeliveries: prev?.recentDeliveries ?? [],
                baseWebhookUrl: prev?.baseWebhookUrl ?? "http://127.0.0.1:6868",
                error: String(err),
              },
            },
          };
        });
        return null;
      }
    },


    async upsertEventTrigger(
      req: UpsertEventTriggerRequest,
    ): Promise<EventTriggersSnapshotView> {
      const hostId = req.host_id;
      const seq = (eventTriggersSeqByHost.get(hostId) ?? 0) + 1;
      eventTriggersSeqByHost.set(hostId, seq);
      try {
        const snap = await orbitIpc.upsertEventTrigger(req);
        if (eventTriggersSeqByHost.get(hostId) === seq) {
          setState((s) => ({
            ...s,
            eventTriggersByHost: {
              ...s.eventTriggersByHost,
              [hostId]: {
                status: "ready",
                triggers: snap.triggers,
                recentDeliveries: snap.recent_deliveries,
                baseWebhookUrl: snap.base_webhook_url,
                error: null,
              },
            },
          }));
        }
        return snap;
      } catch (err) {
        notifyToast(`Trigger save failed: ${String(err)}`);
        throw err;
      }
    },


    async deleteEventTrigger(
      hostId: string,
      triggerId: string,
    ): Promise<EventTriggersSnapshotView> {
      const seq = (eventTriggersSeqByHost.get(hostId) ?? 0) + 1;
      eventTriggersSeqByHost.set(hostId, seq);
      try {
        const snap = await orbitIpc.deleteEventTrigger(hostId, triggerId);
        if (eventTriggersSeqByHost.get(hostId) === seq) {
          setState((s) => ({
            ...s,
            eventTriggersByHost: {
              ...s.eventTriggersByHost,
              [hostId]: {
                status: "ready",
                triggers: snap.triggers,
                recentDeliveries: snap.recent_deliveries,
                baseWebhookUrl: snap.base_webhook_url,
                error: null,
              },
            },
          }));
        }
        notifyToast("Trigger deleted");
        return snap;
      } catch (err) {
        notifyToast(`Trigger delete failed: ${String(err)}`);
        throw err;
      }
    },


    async testEventTrigger(
      hostId: string,
      triggerId: string,
      sampleEventType: string,
      samplePayload: Record<string, unknown>,
    ): Promise<EventTriggerDelivery> {
      try {
        const delivery = await orbitIpc.testEventTrigger(
          hostId,
          triggerId,
          sampleEventType,
          samplePayload,
        );
        setState((s) => {
          const prev = s.eventTriggersByHost[hostId];
          if (!prev) return s;
          const nextDeliveries = [
            delivery,
            ...prev.recentDeliveries.filter((d) => d.delivery_id !== delivery.delivery_id),
          ].slice(0, 50);
          const nextTriggers =
            delivery.status === "dispatched"
              ? prev.triggers.map((t) =>
                  t.trigger_id === triggerId
                    ? {
                        ...t,
                        trigger_count: t.trigger_count + 1,
                        last_triggered_at_ms: delivery.received_at_ms,
                      }
                    : t,
                )
              : prev.triggers;
          return {
            ...s,
            eventTriggersByHost: {
              ...s.eventTriggersByHost,
              [hostId]: {
                ...prev,
                triggers: nextTriggers,
                recentDeliveries: nextDeliveries,
              },
            },
          };
        });
        notifyToast(`${delivery.event_type}: ${delivery.status} · ${delivery.summary}`);
        return delivery;
      } catch (err) {
        notifyToast(`Trigger test failed: ${String(err)}`);
        throw err;
      }
    },


    async loadFileDiagnostics(
      hostId: string,
      cwd: string,
      path: string,
      silent = false,
    ): Promise<LspDiagnostic[]> {
      if (!hostId || !cwd || cwd === "~" || !path) return [];
      const cKey = fileCacheKey(hostId, cwd, path);
      const seq = (lspDiagSeqByKey.get(cKey) ?? 0) + 1;
      lspDiagSeqByKey.set(cKey, seq);
      if (!silent) {
        setState((s) => ({
          ...s,
          lspDiagnosticsByFile: upsertLruRecord(
            s.lspDiagnosticsByFile,
            cKey,
            {
              status: "loading",
              diagnostics: s.lspDiagnosticsByFile[cKey]?.diagnostics ?? [],
            },
            MAX_LSP_DIAGNOSTICS_CACHE_ENTRIES,
          ),
        }));
      }
      try {
        const diagnostics = await orbitIpc.lspGetDiagnostics(hostId, cwd, path);
        if (lspDiagSeqByKey.get(cKey) !== seq) return [];
        setState((s) => ({
          ...s,
          lspDiagnosticsByFile: upsertLruRecord(
            s.lspDiagnosticsByFile,
            cKey,
            { status: "ready", diagnostics },
            MAX_LSP_DIAGNOSTICS_CACHE_ENTRIES,
          ),
        }));
        return diagnostics;
      } catch {
        if (lspDiagSeqByKey.get(cKey) !== seq) return [];
        setState((s) => ({
          ...s,
          lspDiagnosticsByFile: upsertLruRecord(
            s.lspDiagnosticsByFile,
            cKey,
            {
              status: "error",
              diagnostics: s.lspDiagnosticsByFile[cKey]?.diagnostics ?? [],
            },
            MAX_LSP_DIAGNOSTICS_CACHE_ENTRIES,
          ),
        }));
        return [];
      }
    },


    async gotoLspDefinition(
      hostId: string,
      cwd: string,
      path: string,
      line: number,
      col: number,
    ): Promise<LspLocation | null> {
      if (!hostId || !cwd || cwd === "~" || !path || line < 1 || col < 1) return null;
      const seq = ++nextLspGotoSeq;
      try {
        const locations = await orbitIpc.lspGotoDefinition({
          hostId,
          cwd,
          path,
          line,
          col,
        });
        if (seq !== nextLspGotoSeq || !getState().sidePanelOpen || getState().selectedHostId !== hostId) {
          return null;
        }
        const first = locations[0] ?? null;
        if (!first) {
          notifyToast("No definition found");
          return null;
        }
        ctx.actions.openFileInSidePanel(first.path, {
          hostId,
          cwd,
          focusLine: first.line,
          focusEndLine: first.end_line > first.line ? first.end_line : null,
        });
        return first;
      } catch {
        if (seq !== nextLspGotoSeq || !getState().sidePanelOpen || getState().selectedHostId !== hostId) {
          return null;
        }
        notifyToast("No definition found");
        return null;
      }
    },


    async startGoalRun(req: {
      hostId: string;
      agentId: string;
      objective: string;
      criteria?: string[];
      verifyCommand?: string | null;
      maxIterations?: number | null;
      maxDurationSecs?: number | null;
      replaceExisting?: boolean;
      dispatchInitialPrompt?: boolean | null;
      silentToast?: boolean;
    }): Promise<GoalRunRecord | null> {
      try {
        const goal = await orbitIpc.startGoalRun(req);
        setState((s) => {
          const detail = s.hostDetails[req.hostId];
          if (!detail) return s;
          const prev = detail.goal_runs ?? [];
          const next = [
            goal,
            ...prev.filter((g) => g.goal_id !== goal.goal_id),
          ];
          return {
            ...s,
            hostDetails: {
              ...s.hostDetails,
              [req.hostId]: { ...detail, goal_runs: next },
            },
          };
        });
        if (!req.silentToast) {
          notifyToast("Goal loop started");
        }
        void ctx.actions.reloadHostDetail(req.hostId, true);
        return goal;
      } catch (err) {
        notifyToast(`Failed to start goal: ${String(err)}`);
        return null;
      }
    },


    async updateGoalRun(req: {
      hostId: string;
      goalId: string;
      objective?: string | null;
      criteria?: string[] | null;
      verifyCommand?: string | null;
      maxIterations?: number | null;
      maxDurationSecs?: number | null;
    }): Promise<GoalRunRecord | null> {
      try {
        const goal = await orbitIpc.updateGoalRun(req);
        setState((s) => {
          const detail = s.hostDetails[req.hostId];
          if (!detail) return s;
          const prev = detail.goal_runs ?? [];
          const exists = prev.some((g) => g.goal_id === goal.goal_id);
          const next = exists
            ? prev.map((g) => (g.goal_id === goal.goal_id ? goal : g))
            : [goal, ...prev];
          return {
            ...s,
            hostDetails: {
              ...s.hostDetails,
              [req.hostId]: { ...detail, goal_runs: next },
            },
          };
        });
        void ctx.actions.reloadHostDetail(req.hostId, true);
        return goal;
      } catch (err) {
        notifyToast(`Failed to update goal: ${String(err)}`);
        return null;
      }
    },


    async controlGoalRun(
      hostId: string,
      goalId: string,
      action: GoalControlAction,
    ): Promise<GoalRunRecord | null> {
      try {
        const goal = await orbitIpc.controlGoalRun(hostId, goalId, action);
        setState((s) => {
          const detail = s.hostDetails[hostId];
          if (!detail) return s;
          const prev = detail.goal_runs ?? [];
          const exists = prev.some((g) => g.goal_id === goal.goal_id);
          const next = exists
            ? prev.map((g) => (g.goal_id === goal.goal_id ? goal : g))
            : [goal, ...prev];
          return {
            ...s,
            hostDetails: {
              ...s.hostDetails,
              [hostId]: { ...detail, goal_runs: next },
            },
          };
        });
        const label =
          action === "pause"
            ? "Goal paused"
            : action === "resume"
              ? "Goal resumed"
              : action === "cancel"
                ? "Goal stopped"
                : goal.status === "succeeded"
                  ? "Goal verification passed"
                  : "Verification completed";
        notifyToast(label);
        void ctx.actions.reloadHostDetail(hostId, true);
        return goal;
      } catch (err) {
        notifyToast(`Goal action failed: ${String(err)}`);
        return null;
      }
    },


    openGoalEditDialog(hostId: string, goal: GoalRunRecord): void {
      setState((s) => ({
        ...s,
        goalEditDialogTarget: { hostId, goal },
      }));
    },


    closeGoalEditDialog(): void {
      setState((s) => (s.goalEditDialogTarget ? { ...s, goalEditDialogTarget: null } : s));
    },


    openGoalReplaceConfirm(target: {
      hostId: string;
      agentId: string;
      currentGoal: GoalRunRecord;
      newObjective: string;
      onConfirm: () => Promise<void>;
    }): void {
      setState((s) => ({
        ...s,
        goalReplaceConfirmTarget: target,
      }));
    },


    closeGoalReplaceConfirm(): void {
      setState((s) =>
        s.goalReplaceConfirmTarget ? { ...s, goalReplaceConfirmTarget: null } : s,
      );
    },


    async startMonitorWatch(req: {
      hostId: string;
      agentId: string;
      name: string;
      kind: MonitorKind;
      expiresInSecs?: number | null;
    }): Promise<MonitorWatchRecord | null> {
      try {
        const monitor = await orbitIpc.startMonitorWatch(req);
        notifyToast(`Monitor started: ${monitor.name}`);
        void ctx.actions.reloadHostDetail(req.hostId, true);
        return monitor;
      } catch (err) {
        notifyToast(`Failed to start monitor: ${String(err)}`);
        return null;
      }
    },


    async stopMonitorWatch(hostId: string, monitorId: string): Promise<MonitorWatchRecord | null> {
      try {
        const monitor = await orbitIpc.stopMonitorWatch(hostId, monitorId);
        notifyToast(`Stopped monitor: ${monitor.name}`);
        void ctx.actions.reloadHostDetail(hostId, true);
        return monitor;
      } catch (err) {
        notifyToast(`Failed to stop monitor: ${String(err)}`);
        return null;
      }
    },


    async refreshGoalsAndMonitors(hostId: string): Promise<void> {
      if (!hostId) return;
      await Promise.all([
        orbitIpc.listGoalRuns(hostId).catch(() => []),
        orbitIpc.listMonitorWatches(hostId).catch(() => []),
        orbitIpc.listWorkflowRuns(hostId, true).catch(() => []),
      ]);
      await ctx.actions.reloadHostDetail(hostId, true);
    },


    async loadBrowserSessions(hostId: string): Promise<BrowserSessionsSnapshotView | null> {
      if (!hostId) return null;
      const seq = (browserReqSeqByHost.get(hostId) ?? 0) + 1;
      browserReqSeqByHost.set(hostId, seq);
      setState((s) => {
        const prev = s.browserByHost[hostId];
        return {
          ...s,
          browserByHost: {
            ...s.browserByHost,
            [hostId]: {
              status: prev?.status === "ready" ? "ready" : "loading",
              sessions: prev?.sessions ?? [],
              policy: prev?.policy ?? null,
              chromeBinary: prev?.chromeBinary ?? null,
              activeSessionId: prev?.activeSessionId ?? null,
              elementsBySession: prev?.elementsBySession ?? {},
              textExcerptBySession: prev?.textExcerptBySession ?? {},
              busy: prev?.busy ?? false,
              error: null,
            },
          },
        };
      });
      try {
        const snap = await orbitIpc.listBrowserSessions(hostId);
        if (browserReqSeqByHost.get(hostId) !== seq) return null;
        setState((s) => {
          const prev = s.browserByHost[hostId];
          const nextActive =
            prev?.activeSessionId && snap.sessions.some((x) => x.session_id === prev.activeSessionId)
              ? prev.activeSessionId
              : (snap.sessions[0]?.session_id ?? null);
          return {
            ...s,
            browserByHost: {
              ...s.browserByHost,
              [hostId]: {
                status: "ready",
                sessions: snap.sessions,
                policy: snap.policy,
                chromeBinary: snap.chrome_binary ?? null,
                activeSessionId: nextActive,
                elementsBySession: prev?.elementsBySession ?? {},
                textExcerptBySession: prev?.textExcerptBySession ?? {},
                busy: false,
                error: null,
              },
            },
          };
        });
        return snap;
      } catch (err) {
        if (browserReqSeqByHost.get(hostId) !== seq) return null;
        setState((s) => {
          const prev = s.browserByHost[hostId];
          return {
            ...s,
            browserByHost: {
              ...s.browserByHost,
              [hostId]: {
                status: "error",
                sessions: prev?.sessions ?? [],
                policy: prev?.policy ?? null,
                chromeBinary: prev?.chromeBinary ?? null,
                activeSessionId: prev?.activeSessionId ?? null,
                elementsBySession: prev?.elementsBySession ?? {},
                textExcerptBySession: prev?.textExcerptBySession ?? {},
                busy: false,
                error: String(err),
              },
            },
          };
        });
        return null;
      }
    },


    selectBrowserSession(hostId: string, sessionId: string | null) {
      if (!hostId) return;
      setState((s) => {
        const prev = s.browserByHost[hostId];
        if (!prev) return s;
        return {
          ...s,
          browserByHost: {
            ...s.browserByHost,
            [hostId]: {
              ...prev,
              activeSessionId: sessionId,
            },
          },
        };
      });
    },


    async createBrowserSession(req: {
      hostId: string;
      agentId?: string | null;
      initialUrl?: string | null;
      headless?: boolean | null;
      viewportWidth?: number | null;
      viewportHeight?: number | null;
      workspaceRoot?: string | null;
    }): Promise<BrowserSessionSummary | null> {
      if (!req.hostId) return null;
      const seq = (browserReqSeqByHost.get(req.hostId) ?? 0) + 1;
      browserReqSeqByHost.set(req.hostId, seq);
      setState((s) => {
        const prev = s.browserByHost[req.hostId];
        return {
          ...s,
          browserByHost: {
            ...s.browserByHost,
            [req.hostId]: {
              status: prev?.status ?? "loading",
              sessions: prev?.sessions ?? [],
              policy: prev?.policy ?? null,
              chromeBinary: prev?.chromeBinary ?? null,
              activeSessionId: prev?.activeSessionId ?? null,
              elementsBySession: prev?.elementsBySession ?? {},
              textExcerptBySession: prev?.textExcerptBySession ?? {},
              busy: true,
              error: null,
            },
          },
        };
      });
      try {
        const session = await orbitIpc.createBrowserSession(req);
        setState((s) => {
          const prev = s.browserByHost[req.hostId];
          const nextSessions = [
            session,
            ...(prev?.sessions ?? []).filter((x) => x.session_id !== session.session_id),
          ];
          return {
            ...s,
            browserByHost: {
              ...s.browserByHost,
              [req.hostId]: {
                status: "ready",
                sessions: nextSessions,
                policy: prev?.policy ?? null,
                chromeBinary: prev?.chromeBinary ?? null,
                activeSessionId: session.session_id,
                elementsBySession: prev?.elementsBySession ?? {},
                textExcerptBySession: prev?.textExcerptBySession ?? {},
                busy: false,
                error: null,
              },
            },
          };
        });
        // Immediately capture an initial viewport screenshot for visual feedback.
        void ctx.actions.executeBrowserAction(req.hostId, session.session_id, {
          kind: "screenshot",
          full_page: false,
        });
        return session;
      } catch (err) {
        const msg = String(err);
        setState((s) => {
          const prev = s.browserByHost[req.hostId];
          if (!prev) return s;
          return {
            ...s,
            browserByHost: {
              ...s.browserByHost,
              [req.hostId]: { ...prev, busy: false, error: msg },
            },
          };
        });
        notifyToast(`CDP session error: ${msg}`);
        return null;
      }
    },


    async executeBrowserAction(
      hostId: string,
      sessionId: string,
      action: BrowserActionRequest,
    ): Promise<BrowserActionResponseView | null> {
      if (!hostId || !sessionId) return null;
      setState((s) => {
        const prev = s.browserByHost[hostId];
        if (!prev) return s;
        return {
          ...s,
          browserByHost: {
            ...s.browserByHost,
            [hostId]: { ...prev, busy: true, error: null },
          },
        };
      });
      try {
        const res = await orbitIpc.executeBrowserAction({ hostId, sessionId, action });
        setState((s) => {
          const prev = s.browserByHost[hostId];
          if (!prev) return s;
          const existingSession = prev.sessions.find((x) => x.session_id === res.session.session_id);
          // Do not resurrect a session that was closed while this action was in flight.
          if (!existingSession) {
            return {
              ...s,
              browserByHost: {
                ...s.browserByHost,
                [hostId]: { ...prev, busy: false },
              },
            };
          }
          const mergedSession: BrowserSessionSummary = {
            ...res.session,
            last_screenshot_base64:
              res.screenshot_base64 ??
              res.session.last_screenshot_base64 ??
              existingSession.last_screenshot_base64 ??
              null,
          };
          const nextSessions = prev.sessions.map((x) =>
            x.session_id === mergedSession.session_id ? mergedSession : x,
          );
          const nextActive =
            prev.activeSessionId && nextSessions.some((x) => x.session_id === prev.activeSessionId)
              ? prev.activeSessionId
              : (nextSessions[0]?.session_id ?? null);
          return {
            ...s,
            browserByHost: {
              ...s.browserByHost,
              [hostId]: {
                status: "ready",
                sessions: nextSessions,
                policy: prev.policy,
                chromeBinary: prev.chromeBinary,
                activeSessionId: nextActive,
                elementsBySession: {
                  ...prev.elementsBySession,
                  [mergedSession.session_id]: res.elements,
                },
                textExcerptBySession: {
                  ...prev.textExcerptBySession,
                  [mergedSession.session_id]:
                    res.text_excerpt !== undefined
                      ? res.text_excerpt
                      : (prev.textExcerptBySession[mergedSession.session_id] ?? null),
                },
                busy: false,
                error: null,
              },
            },
          };
        });
        return res;
      } catch (err) {
        const msg = String(err);
        setState((s) => {
          const prev = s.browserByHost[hostId];
          if (!prev) return s;
          return {
            ...s,
            browserByHost: {
              ...s.browserByHost,
              [hostId]: { ...prev, busy: false, error: msg },
            },
          };
        });
        notifyToast(msg);
        return null;
      }
    },


    async closeBrowserSession(
      hostId: string,
      sessionId: string,
    ): Promise<BrowserSessionsSnapshotView | null> {
      if (!hostId || !sessionId) return null;
      const seq = (browserReqSeqByHost.get(hostId) ?? 0) + 1;
      browserReqSeqByHost.set(hostId, seq);
      setState((s) => {
        const prev = s.browserByHost[hostId];
        if (!prev) return s;
        return {
          ...s,
          browserByHost: {
            ...s.browserByHost,
            [hostId]: { ...prev, busy: true, error: null },
          },
        };
      });
      try {
        const snap = await orbitIpc.closeBrowserSession(hostId, sessionId);
        if (browserReqSeqByHost.get(hostId) !== seq) return snap;
        setState((s) => {
          const prev = s.browserByHost[hostId];
          const nextElements = { ...(prev?.elementsBySession ?? {}) };
          delete nextElements[sessionId];
          const nextExcerpts = { ...(prev?.textExcerptBySession ?? {}) };
          delete nextExcerpts[sessionId];
          const nextActive =
            prev?.activeSessionId &&
            prev.activeSessionId !== sessionId &&
            snap.sessions.some((x) => x.session_id === prev.activeSessionId)
              ? prev.activeSessionId
              : (snap.sessions[0]?.session_id ?? null);
          return {
            ...s,
            browserByHost: {
              ...s.browserByHost,
              [hostId]: {
                status: "ready",
                sessions: snap.sessions,
                policy: snap.policy,
                chromeBinary: snap.chrome_binary ?? null,
                activeSessionId: nextActive,
                elementsBySession: nextElements,
                textExcerptBySession: nextExcerpts,
                busy: false,
                error: null,
              },
            },
          };
        });
        notifyToast("CDP session closed");
        return snap;
      } catch (err) {
        setState((s) => {
          const prev = s.browserByHost[hostId];
          if (!prev) return s;
          return {
            ...s,
            browserByHost: {
              ...s.browserByHost,
              [hostId]: { ...prev, busy: false, error: String(err) },
            },
          };
        });
        notifyToast(`Failed to close CDP session: ${String(err)}`);
        return null;
      }
    },


    async updateBrowserPolicy(
      hostId: string,
      policy: BrowserPolicyConfig,
    ): Promise<BrowserSessionsSnapshotView | null> {
      if (!hostId) return null;
      const seq = (browserReqSeqByHost.get(hostId) ?? 0) + 1;
      browserReqSeqByHost.set(hostId, seq);
      try {
        const snap = await orbitIpc.updateBrowserPolicy(hostId, policy);
        if (browserReqSeqByHost.get(hostId) !== seq) return snap;
        setState((s) => {
          const prev = s.browserByHost[hostId];
          return {
            ...s,
            browserByHost: {
              ...s.browserByHost,
              [hostId]: {
                status: "ready",
                sessions: snap.sessions,
                policy: snap.policy,
                chromeBinary: snap.chrome_binary ?? null,
                activeSessionId:
                  prev?.activeSessionId &&
                  snap.sessions.some((x) => x.session_id === prev.activeSessionId)
                    ? prev.activeSessionId
                    : (snap.sessions[0]?.session_id ?? null),
                elementsBySession: prev?.elementsBySession ?? {},
                textExcerptBySession: prev?.textExcerptBySession ?? {},
                busy: false,
                error: null,
              },
            },
          };
        });
        notifyToast("Browser policy updated");
        return snap;
      } catch (err) {
        notifyToast(`Failed to update browser policy: ${String(err)}`);
        return null;
      }
    },


    async startBatchFanout(req: {
      hostId: string;
      name: string;
      cwd: string;
      csvContent: string;
      promptTemplate: string;
      provider: ProviderKind;
      model?: string | null;
      mode: SessionMode;
      maxConcurrency?: number | null;
      isolatedWorktree?: boolean;
    }): Promise<WorkflowRunRecord | null> {
      try {
        const run = await orbitIpc.startBatchFanout(req);
        notifyToast(`Batch started: ${run.name}`);
        void ctx.actions.reloadHostDetail(req.hostId, true);
        return run;
      } catch (err) {
        notifyToast(`Failed to start batch: ${String(err)}`);
        return null;
      }
    },


    async startDagWorkflow(req: {
      hostId: string;
      name: string;
      cwd: string;
      steps: DagStepSpec[];
      defaultProvider: ProviderKind;
      defaultMode: SessionMode;
      maxConcurrency?: number | null;
    }): Promise<WorkflowRunRecord | null> {
      try {
        const run = await orbitIpc.startDagWorkflow(req);
        notifyToast(`DAG started: ${run.name}`);
        void ctx.actions.reloadHostDetail(req.hostId, true);
        return run;
      } catch (err) {
        notifyToast(`Failed to start DAG: ${String(err)}`);
        return null;
      }
    },


    async startBestOfN(req: {
      hostId: string;
      name: string;
      cwd: string;
      prompt: string;
      candidates: BestOfNCandidateSpec[];
      mode: SessionMode;
      verifyCommand?: string | null;
    }): Promise<WorkflowRunRecord | null> {
      try {
        const run = await orbitIpc.startBestOfN(req);
        notifyToast(`Arena started: ${run.name}`);
        void ctx.actions.reloadHostDetail(req.hostId, true);
        return run;
      } catch (err) {
        notifyToast(`Failed to start Arena: ${String(err)}`);
        return null;
      }
    },


    async cancelWorkflowRun(hostId: string, runId: string): Promise<WorkflowRunRecord | null> {
      try {
        const run = await orbitIpc.cancelWorkflowRun(hostId, runId);
        notifyToast(`Canceled workflow: ${run.name}`);
        void ctx.actions.reloadHostDetail(hostId, true);
        return run;
      } catch (err) {
        notifyToast(`Failed to cancel workflow: ${String(err)}`);
        return null;
      }
    },


    async selectBestOfNWinner(
      hostId: string,
      runId: string,
      winnerStepId: string,
      applyToWorkspace = true,
    ): Promise<WorkflowRunRecord | null> {
      try {
        const run = await orbitIpc.selectBestOfNWinner(hostId, runId, winnerStepId, applyToWorkspace);
        notifyToast(applyToWorkspace ? "Winner applied to workspace" : "Winner selected");
        void ctx.actions.reloadHostDetail(hostId, true);
        return run;
      } catch (err) {
        notifyToast(`Failed to apply winner: ${String(err)}`);
        return null;
      }
    },


    async refreshWorkflowRuns(hostId: string): Promise<void> {
      if (!hostId) return;
      await orbitIpc.listWorkflowRuns(hostId, true).catch(() => []);
      await ctx.actions.reloadHostDetail(hostId, true);
    },


    async refreshExternalMcpServers(
      hostId: string,
      cwd?: string | null,
    ): Promise<ExternalMcpServerRecord[]> {
      const seq = (externalMcpSnapshotSeqByHost[hostId] ?? 0) + 1;
      externalMcpSnapshotSeqByHost[hostId] = seq;
      const servers = await orbitIpc.listExternalMcpServers(hostId, cwd);
      if (externalMcpSnapshotSeqByHost[hostId] === seq) {
        setState((s) => ({
          ...s,
          externalMcpServersByHost: {
            ...s.externalMcpServersByHost,
            [hostId]: servers,
          },
        }));
      }
      return servers;
    },


    async upsertExternalMcpServer(
      request: UpsertExternalMcpServerRequest,
    ): Promise<ExternalMcpServerRecord[]> {
      const seq = (externalMcpSnapshotSeqByHost[request.host_id] ?? 0) + 1;
      externalMcpSnapshotSeqByHost[request.host_id] = seq;
      const servers = await orbitIpc.upsertExternalMcpServer(request);
      if (externalMcpSnapshotSeqByHost[request.host_id] === seq) {
        setState((s) => ({
          ...s,
          externalMcpServersByHost: {
            ...s.externalMcpServersByHost,
            [request.host_id]: servers,
          },
        }));
      }
      return servers;
    },


    async toggleExternalMcpServer(
      hostId: string,
      serverId: string,
      enabled: boolean,
      cwd?: string | null,
    ): Promise<ExternalMcpServerRecord[]> {
      const seq = (externalMcpSnapshotSeqByHost[hostId] ?? 0) + 1;
      externalMcpSnapshotSeqByHost[hostId] = seq;
      const servers = await orbitIpc.toggleExternalMcpServer(hostId, serverId, enabled, cwd);
      if (externalMcpSnapshotSeqByHost[hostId] === seq) {
        setState((s) => ({
          ...s,
          externalMcpServersByHost: {
            ...s.externalMcpServersByHost,
            [hostId]: servers,
          },
        }));
      }
      return servers;
    },


    async deleteExternalMcpServer(
      hostId: string,
      serverId: string,
      cwd?: string | null,
    ): Promise<ExternalMcpServerRecord[]> {
      const seq = (externalMcpSnapshotSeqByHost[hostId] ?? 0) + 1;
      externalMcpSnapshotSeqByHost[hostId] = seq;
      const servers = await orbitIpc.deleteExternalMcpServer(hostId, serverId, cwd);
      if (externalMcpSnapshotSeqByHost[hostId] === seq) {
        setState((s) => ({
          ...s,
          externalMcpServersByHost: {
            ...s.externalMcpServersByHost,
            [hostId]: servers,
          },
        }));
      }
      return servers;
    },


    async probeExternalMcpServer(
      hostId: string,
      serverId: string,
      cwd?: string | null,
    ): Promise<McpProbeResult> {
      const result = await orbitIpc.probeExternalMcpServer(hostId, serverId, cwd);
      setState((s) => {
        const existing = s.externalMcpServersByHost[hostId];
        if (!existing) return s;
        return {
          ...s,
          externalMcpServersByHost: {
            ...s.externalMcpServersByHost,
            [hostId]: existing.map((srv) =>
              srv.id === serverId ? { ...srv, last_probe: result } : srv,
            ),
          },
        };
      });
      return result;
    },


    async refreshPathRules(hostId: string, cwd: string): Promise<PathRulesSnapshotView | null> {
      if (!hostId || !cwd) return null;
      const scopeKey = `${hostId}::${cwd}`;
      const reqSeq = (pathRulesReqSeqByScope.get(scopeKey) ?? 0) + 1;
      pathRulesReqSeqByScope.set(scopeKey, reqSeq);
      try {
        const snap = await orbitIpc.listPathRules(hostId, cwd);
        if (pathRulesReqSeqByScope.get(scopeKey) === reqSeq) {
          setState((s) => ({
            ...s,
            pathRulesByScope: {
              ...s.pathRulesByScope,
              [scopeKey]: snap,
            },
          }));
        }
        return snap;
      } catch {
        return null;
      }
    },


    async upsertPathRule(params: UpsertPathRuleParams): Promise<PathRulesSnapshotView> {
      const scopeKey = `${params.hostId}::${params.cwd}`;
      const reqSeq = (pathRulesReqSeqByScope.get(scopeKey) ?? 0) + 1;
      pathRulesReqSeqByScope.set(scopeKey, reqSeq);
      const snap = await orbitIpc.upsertPathRule(params);
      if (pathRulesReqSeqByScope.get(scopeKey) === reqSeq) {
        setState((s) => ({
          ...s,
          pathRulesByScope: {
            ...s.pathRulesByScope,
            [scopeKey]: snap,
          },
        }));
      }
      return snap;
    },


    async deletePathRule(params: {
      hostId: string;
      cwd: string;
      path: string;
    }): Promise<PathRulesSnapshotView> {
      const scopeKey = `${params.hostId}::${params.cwd}`;
      const reqSeq = (pathRulesReqSeqByScope.get(scopeKey) ?? 0) + 1;
      pathRulesReqSeqByScope.set(scopeKey, reqSeq);
      const snap = await orbitIpc.deletePathRule(params);
      if (pathRulesReqSeqByScope.get(scopeKey) === reqSeq) {
        setState((s) => ({
          ...s,
          pathRulesByScope: {
            ...s.pathRulesByScope,
            [scopeKey]: snap,
          },
        }));
      }
      return snap;
    },


    async matchPathRules(params: {
      hostId: string;
      cwd: string;
      activePaths: string[];
      manualRuleNames: string[];
    }): Promise<MatchedPathRulesView> {
      return orbitIpc.matchPathRules(params);
    },
  };
}
