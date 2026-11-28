import { describe, expect, it } from '@ohos/hypium';
import { OrbitCoreBridge } from '../../../main/ets/model/OrbitCoreBridge';
import { OrbitHarmonyStore } from '../../../main/ets/model/OrbitHarmonyStore';
import { OrbitKeyValuePrefsBackend } from '../../../main/ets/model/OrbitPreferences';
import {
  OrbitHapticPreset,
  OrbitNativeHapticDriver,
  normalizeOrbitAppLanguage,
  normalizeOrbitHapticPreset,
  resolveEffectiveOrbitLanguage,
  resolveNativeHapticEffectId,
  triggerNativeHapticWithDriver
} from '../../../main/ets/model/OrbitDeviceBridge';
import {
  orbitIsLegacyThemeMode,
  orbitNormalizeThemeMode
} from '../../../main/ets/model/OrbitWindowTheme';
import { OrbitTheme, OrbitTokens } from '../../../main/ets/theme/OrbitTheme';
import {
  ORBIT_APPEARANCE_MODES,
  ORBIT_SETTINGS_GROUPS,
  OrbitMcpRowBinding,
  OrbitMemoryCwdOptionBinding,
  OrbitMemoryRowBinding,
  OrbitPluginRowBinding,
  OrbitPullRefreshController,
  OrbitSettingsRequestGuard,
  OrbitSettingsSearchHit,
  applySettledMcpToggle,
  applySettledMemoryConfirm,
  applySettledMemoryDelete,
  applySettledPluginToggle,
  canSettleMcpRowWrite,
  canSettleMemoryRowWrite,
  canSettlePluginRowWrite,
  createMemoryCwdOptionBinding,
  createSettingsControlBinding,
  isMcpRowBindingActive,
  isMemoryCwdOptionBindingActive,
  isMemoryRowBindingActive,
  isPluginRowBindingActive,
  isSettingsControlBindingActive,
  localizeSettingsSearchGroup,
  localizeSettingsSearchTitle,
  migrateLegacyAppearanceMode,
  resolveEffectiveMemoryCwd,
  resolveHostScopedExtensionCwdPure,
  resolveOrbitAppearanceIndex,
  resolveOrbitSettingsInitialDestination,
  searchOrbitSettings
} from '../../../main/ets/components/settings/OrbitSettingsNavModel';
import {
  ExternalMcpServerRecordDto,
  InstalledPluginDto,
  OrbitScopedActionOutcome,
  ProjectMemoryItemDto,
  UsageInsightsSnapshotDto
} from '../../../main/ets/model/OrbitTypes';

interface DeferredStringResult {
  promise: Promise<string>;
  resolve: (value: string) => void;
  reject: (err: Error) => void;
}

function createDeferredString(): DeferredStringResult {
  let resFn: (value: string) => void = (_v: string) => {};
  let rejFn: (err: Error) => void = (_e: Error) => {};
  const promise = new Promise<string>((resolve: (v: string) => void, reject: (e: Error) => void) => {
    resFn = resolve;
    rejFn = reject;
  });
  return { promise: promise, resolve: resFn, reject: rejFn };
}

class FakePr6CoreBridge extends OrbitCoreBridge {
  public fleetJson: string = '{"revision":1,"hosts":[],"projects":[],"total_pending_approvals":0}';
  public sidebarJson: string = '{"revision":1,"projects":[],"launchpad_projects":[],"needs_attention":[],"pinned_conversations":[]}';
  public exportPairCalls: string[] = [];
  public exportPairResult: string = 'orbit://pair/test-payload-pr6';
  public exportPairError: string = '';

  public applyRelayCalls: string[] = [];
  public applyRelayResult: number = 2;

  public usageCalls: string[] = [];
  public usageJsonByHost: Map<string, string> = new Map<string, string>();
  public usageDeferredByHost: Map<string, DeferredStringResult> = new Map<string, DeferredStringResult>();
  public usageError: string = '';

  public listMcpCalls: string[] = [];
  public listMcpJson: string = '[{"id":"orbit_user:fs","name":"filesystem","scope":"orbit_user","transport":{"kind":"stdio","command":"npx","args":["-y","fs"]},"env_keys":[],"masked_env":{},"masked_headers":{},"enabled":true,"editable":true,"source_path":"~/.orbit/mcp.json","last_probe":null}]';
  public toggleMcpCalls: string[] = [];
  public toggleMcpError: string = '';

  public listSkillsCalls: string[] = [];
  public listSkillsJson: string = '[{"id":"orbit_user:review","slug":"review","name":"Review","description":"PR review","source":"orbit_user","path":"~/.orbit/skills/review/SKILL.md","enabled":true,"editable":true,"triggers":[],"allowed_tools":[],"has_scripts":false,"has_references":false,"approx_tokens":320,"updated_at_ms":1000,"argument_hint":null,"is_command":false}]';

  public listMemoriesCalls: string[] = [];
  public listMemoriesJson: string = '[{"id":"mem-1","workspace_root":"/repo","title":"Use tokens","content":"Use $r app.color","category":"convention","status":"hypothesis","confidence":0.9,"file_paths":[],"tags":[],"source_provider":"claude","source_agent_id":"ag-1","source_path":null,"access_count":1,"created_at_ms":1,"updated_at_ms":1,"last_accessed_ms":1}]';
  public confirmMemoryCalls: string[] = [];
  public confirmMemoryError: string = '';
  public deleteMemoryCalls: string[] = [];
  public deleteMemoryError: string = '';

  public listPluginsCalls: string[] = [];
  public listPluginsResponseJson: string = '[{"manifest":{"id":"orbit-lint","name":"Orbit Lint","version":"1.0.0","description":"Lint","author":"Orbit","wasm_entry":"plugin.wasm","capabilities":{"fs_read_globs":[],"http_domains":[],"hook_events":[]},"mcp_ops":[],"ui_slots":[]},"enabled":true}]';
  public setPluginEnabledCalls: string[] = [];
  public setPluginEnabledError: string = '';

  public getSandboxCalls: string[] = [];
  public getSandboxResponseJson: string = '{"mode":"workspace_write","writable_roots":["/repo"],"denied_read_paths":["~/.ssh"],"allow_network":false,"allowed_domains":[],"denied_domains":[]}';
  public getSandboxError: string = '';

  public testPushCalls: string[] = [];
  public testPushError: string = '';
  public switchAgentProviderCalls: string[] = [];
  public setAgentConfigCalls: string[] = [];
  public hostDetailByHost: Map<string, string> = new Map<string, string>();

  public connectAllCalls: number = 0;

  constructor() {
    super(null, false);
  }

  public override subscribeEvents(_callback: (err: Error | null, payloadJson: string) => void): void {}
  public override fallbackProviderCatalogsJson(): string { return '[]'; }
  public override slashCommandsJson(_platform?: string | null, _query?: string | null): string { return '[]'; }
  public override cloudSyncStatusJson(): string {
    return '{"enabled":false,"configured":false,"state":"signed_out","device_id":"","last_sync_at":null,"last_error":null}';
  }
  public override fleetOverviewJson(): string { return this.fleetJson; }
  public override async fleetOverviewJsonAsync(): Promise<string> { return this.fleetJson; }
  public override fleetSidebarByRequestJson(_reqJson: string): string { return this.sidebarJson; }
  public override async fleetSidebarByRequestJsonAsync(_reqJson: string): Promise<string> { return this.sidebarJson; }
  public override hostDetailJson(hostId: string): string {
    return this.hostDetailByHost.get(hostId) ?? 'null';
  }
  public override async hostDetailJsonAsync(hostId: string): Promise<string> {
    return this.hostDetailByHost.get(hostId) ?? 'null';
  }
  public override switchAgentProvider(
    hostId: string,
    agentId: string,
    provider: string,
    model: string | null,
    effort: string | null,
    mode: string | null
  ): void {
    this.switchAgentProviderCalls.push(
      `${hostId}|${agentId}|${provider}|${model ?? ''}|${effort ?? ''}|${mode ?? ''}`
    );
  }
  public override setAgentConfig(
    hostId: string,
    agentId: string,
    model: string | null,
    effort: string | null
  ): void {
    this.setAgentConfigCalls.push(`${hostId}|${agentId}|${model ?? ''}|${effort ?? ''}`);
  }
  public override connectAllHosts(): void { this.connectAllCalls += 1; }
  public override connectHost(_hostId: string): void {}
  public override refreshHost(_hostId: string): void {}
  public override refreshQuotas(_hostId: string): void {}
  public override refreshProviderModels(_hostId: string, _force?: boolean): void {}
  public override async discoverFleetRouterHostsJson(): Promise<string> { return '[]'; }
  public override async getFleetMeshSnapshotJson(): Promise<string> { return '{}'; }

  public override exportHostPairingUri(hostId: string): string {
    this.exportPairCalls.push(hostId);
    if (this.exportPairError.length > 0) {
      throw new Error(this.exportPairError);
    }
    return this.exportPairResult;
  }

  public override applyRelayToFleet(rawRelayUrl: string): number {
    this.applyRelayCalls.push(rawRelayUrl);
    return this.applyRelayResult;
  }

  public override async getUsageInsightsJson(hostId: string, days: number): Promise<string> {
    this.usageCalls.push(`${hostId}:${days}`);
    if (this.usageError.length > 0) {
      throw new Error(this.usageError);
    }
    const deferred = this.usageDeferredByHost.get(hostId);
    if (deferred !== undefined) {
      return await deferred.promise;
    }
    const custom = this.usageJsonByHost.get(hostId);
    if (custom !== undefined) {
      return custom;
    }
    return `{"days":${days},"total_cost_usd":2.5,"total_cache_savings_usd":0.4,"total_input_tokens":50000,"total_output_tokens":10000,"total_cache_read_tokens":20000,"total_turns":8,"daily":[],"by_model":[],"by_project":[],"forecasts":[]}`;
  }

  public override async listExternalMcpServersJson(hostId: string, cwd?: string | null): Promise<string> {
    this.listMcpCalls.push(`${hostId}|${cwd ?? ''}`);
    return this.listMcpJson;
  }

  public override async toggleExternalMcpServerJson(
    hostId: string,
    serverId: string,
    enabled: boolean,
    cwd?: string | null
  ): Promise<void> {
    this.toggleMcpCalls.push(`${hostId}|${serverId}|${enabled ? '1' : '0'}|${cwd ?? ''}`);
    if (this.toggleMcpError.length > 0) {
      throw new Error(this.toggleMcpError);
    }
  }

  public override async listManagedSkillsJson(hostId: string, cwd?: string | null): Promise<string> {
    this.listSkillsCalls.push(`${hostId}|${cwd ?? ''}`);
    return this.listSkillsJson;
  }

  public override async listProjectMemoriesJson(hostId: string, cwd: string, limit: number): Promise<string> {
    this.listMemoriesCalls.push(`${hostId}|${cwd}|${limit}`);
    return this.listMemoriesJson;
  }

  public override async confirmProjectMemoryJson(
    hostId: string,
    memoryId: string,
    cwd?: string | null
  ): Promise<void> {
    this.confirmMemoryCalls.push(`${hostId}|${memoryId}|${cwd ?? ''}`);
    if (this.confirmMemoryError.length > 0) {
      throw new Error(this.confirmMemoryError);
    }
  }

  public override async deleteProjectMemoryJson(
    hostId: string,
    memoryId: string,
    cwd?: string | null
  ): Promise<void> {
    this.deleteMemoryCalls.push(`${hostId}|${memoryId}|${cwd ?? ''}`);
    if (this.deleteMemoryError.length > 0) {
      throw new Error(this.deleteMemoryError);
    }
  }

  public override async listPluginsJson(hostId: string): Promise<string> {
    this.listPluginsCalls.push(hostId);
    return this.listPluginsResponseJson;
  }

  public override async setPluginEnabledJson(
    hostId: string,
    pluginId: string,
    enabled: boolean
  ): Promise<void> {
    this.setPluginEnabledCalls.push(`${hostId}|${pluginId}|${enabled ? '1' : '0'}`);
    if (this.setPluginEnabledError.length > 0) {
      throw new Error(this.setPluginEnabledError);
    }
  }

  public override async getSandboxConfigJson(hostId: string): Promise<string> {
    this.getSandboxCalls.push(hostId);
    if (this.getSandboxError.length > 0) {
      throw new Error(this.getSandboxError);
    }
    return this.getSandboxResponseJson;
  }

  public override async testPushNotificationJson(hostId: string, deviceId: string): Promise<string> {
    this.testPushCalls.push(`${hostId}|${deviceId}`);
    if (this.testPushError.length > 0) {
      throw new Error(this.testPushError);
    }
    return '{}';
  }
}

class InMemoryPr6PrefsBackend implements OrbitKeyValuePrefsBackend {
  public readonly backing: Map<string, string> = new Map<string, string>();
  public putCount: number = 0;
  public flushCount: number = 0;

  public getStringSync(key: string, defaultValue: string): string {
    const val = this.backing.get(key);
    return val !== undefined ? val : defaultValue;
  }

  public putStringSync(key: string, value: string): void {
    this.putCount += 1;
    this.backing.set(key, value);
  }

  public async flushAsync(): Promise<void> {
    this.flushCount += 1;
  }
}

function createPr6TestStore(bridge: FakePr6CoreBridge): OrbitHarmonyStore {
  const store = new OrbitHarmonyStore();
  store.initializeWithTestBridge(bridge);
  return store;
}

export default function orbitPr6Test(): void {
  describe('OrbitPr6SettingsAndPolishSuite', () => {
    it('covers all 11 Tier-2 store RPCs with structured outcomes and zero global toast pollution', 0, async () => {
      const bridge = new FakePr6CoreBridge();
      const store = createPr6TestStore(bridge);
      store.toastMessage = '';
      store.lastError = '';

      // 1. exportHostPairingUriScoped
      const pairOk = store.exportHostPairingUriScoped('h1');
      expect(pairOk.ok).assertTrue();
      expect(pairOk.data).assertEqual('orbit://pair/test-payload-pr6');
      expect(bridge.exportPairCalls[0]).assertEqual('h1');

      bridge.exportPairError = 'no pairing secret';
      const pairFail = store.exportHostPairingUriScoped('h1');
      expect(pairFail.ok).assertFalse();
      expect(pairFail.errorDetail).assertContain('no pairing secret');
      expect(store.toastMessage).assertEqual('');
      expect(store.lastError).assertEqual('');
      bridge.exportPairError = '';

      // 2. getUsageInsightsScoped
      const usageRes = await store.getUsageInsightsScoped('h1', 30);
      expect(usageRes.ok).assertTrue();
      expect(usageRes.data !== null).assertTrue();
      expect(usageRes.data!.days).assertEqual(30);
      expect(bridge.usageCalls[0]).assertEqual('h1:30');

      // 3 & 4. listExternalMcpServersScoped & toggleExternalMcpServerScoped
      const mcpList = await store.listExternalMcpServersScoped('h1', '/repo');
      expect(mcpList.ok).assertTrue();
      expect(mcpList.data!.length).assertEqual(1);
      expect(mcpList.data![0].name).assertEqual('filesystem');

      const mcpToggle = await store.toggleExternalMcpServerScoped('h1', 'orbit_user:fs', false, '/repo');
      expect(mcpToggle.ok).assertTrue();
      expect(bridge.toggleMcpCalls[0]).assertEqual('h1|orbit_user:fs|0|/repo');

      bridge.toggleMcpError = 'host disconnected';
      const mcpToggleFail = await store.toggleExternalMcpServerScoped('h1', 'orbit_user:fs', true, null);
      expect(mcpToggleFail.ok).assertFalse();
      expect(store.toastMessage).assertEqual('');
      bridge.toggleMcpError = '';

      // 5. listManagedSkillsScoped
      const skillsRes = await store.listManagedSkillsScoped('h1', '/repo');
      expect(skillsRes.ok).assertTrue();
      expect(skillsRes.data!.length).assertEqual(1);
      expect(skillsRes.data![0].slug).assertEqual('review');

      // 6, 7 & 8. listProjectMemoriesScoped, confirmProjectMemoryScoped, deleteProjectMemoryScoped
      const memList = await store.listProjectMemoriesScoped('h1', '/repo', 50);
      expect(memList.ok).assertTrue();
      expect(memList.data!.length).assertEqual(1);
      expect(bridge.listMemoriesCalls[0]).assertEqual('h1|/repo|50');

      const memConfirm = await store.confirmProjectMemoryScoped('h1', 'mem-1', '/repo');
      expect(memConfirm.ok).assertTrue();
      expect(bridge.confirmMemoryCalls[0]).assertEqual('h1|mem-1|/repo');

      const memDelete = await store.deleteProjectMemoryScoped('h1', 'mem-1', '/repo');
      expect(memDelete.ok).assertTrue();
      expect(bridge.deleteMemoryCalls[0]).assertEqual('h1|mem-1|/repo');

      // 9 & 10. listPluginsScoped & setPluginEnabledScoped
      const pluginsRes = await store.listPluginsScoped('h1');
      expect(pluginsRes.ok).assertTrue();
      expect(pluginsRes.data!.length).assertEqual(1);
      expect(pluginsRes.data![0].manifest.id).assertEqual('orbit-lint');

      const pluginToggle = await store.setPluginEnabledScoped('h1', 'orbit-lint', false);
      expect(pluginToggle.ok).assertTrue();
      expect(bridge.setPluginEnabledCalls[0]).assertEqual('h1|orbit-lint|0');

      // 11. getSandboxConfigScoped (both active policy and null default)
      const sandboxRes = await store.getSandboxConfigScoped('h1');
      expect(sandboxRes.ok).assertTrue();
      expect(sandboxRes.data !== null).assertTrue();
      expect(sandboxRes.data!.mode).assertEqual('workspace_write');

      bridge.getSandboxResponseJson = 'null';
      const sandboxNull = await store.getSandboxConfigScoped('h1');
      expect(sandboxNull.ok).assertTrue();
      expect(sandboxNull.data === null).assertTrue();

      // Fleet Relay one-click sync
      const relayRes = store.applyRelayToFleetScoped('wss://relay.orbit.dev');
      expect(relayRes.ok).assertTrue();
      expect(relayRes.data).assertEqual(2);
      expect(bridge.applyRelayCalls[0]).assertEqual('wss://relay.orbit.dev');

      // Zero global toast/lastError pollution across all 11+ scoped actions
      expect(store.toastMessage).assertEqual('');
      expect(store.lastError).assertEqual('');
    });

    it('enforces four-group Navigation settings architecture and zero Sheet-inside-Sheet routing', 0, () => {
      expect(ORBIT_SETTINGS_GROUPS.length).assertEqual(4);
      expect(ORBIT_SETTINGS_GROUPS[0].id).assertEqual('preferences');
      expect(ORBIT_SETTINGS_GROUPS[1].id).assertEqual('fleet_sync');
      expect(ORBIT_SETTINGS_GROUPS[2].id).assertEqual('agents_safety');
      expect(ORBIT_SETTINGS_GROUPS[3].id).assertEqual('context_extensions');

      expect(resolveOrbitSettingsInitialDestination('router_mesh')).assertEqual('personal_agent');
      expect(resolveOrbitSettingsInitialDestination('push_settings')).assertEqual('notifications');
      expect(resolveOrbitSettingsInitialDestination('add_host')).assertEqual('pair_host');
      expect(resolveOrbitSettingsInitialDestination('sandbox')).assertEqual('sandbox');
      expect(resolveOrbitSettingsInitialDestination('unknown_route')).assertEqual('');

      const bridge = new FakePr6CoreBridge();
      const store = createPr6TestStore(bridge);
      store.fleetStore.setSelectedHostId('h1');

      // Personal Agent & Mesh routes directly into Settings Navigation stack (no nested sheet!)
      store.openRouterMeshSheet();
      const route = store.shellStore.modalRoute;
      expect(route !== null).assertTrue();
      expect(route!.kind).assertEqual('host_settings');
      expect(route!.initialPath).assertEqual('personal_agent');

      store.openHostSettings('mcp', 'h2');
      const nextRoute = store.shellStore.pendingModalRoute ?? store.shellStore.modalRoute;
      expect(nextRoute !== null).assertTrue();
      expect(nextRoute!.kind).assertEqual('host_settings');
      expect(nextRoute!.initialPath).assertEqual('mcp');
      expect(nextRoute!.hostId).assertEqual('h2');
    });

    it('restricts Appearance strictly to System / Light / Dark and migrates legacy graphite/oled safely', 0, () => {
      expect(ORBIT_APPEARANCE_MODES.length).assertEqual(3);
      expect(ORBIT_APPEARANCE_MODES.includes('graphite')).assertFalse();
      expect(ORBIT_APPEARANCE_MODES.includes('oled')).assertFalse();

      expect(orbitIsLegacyThemeMode('graphite')).assertTrue();
      expect(orbitIsLegacyThemeMode('OLED')).assertTrue();
      expect(orbitIsLegacyThemeMode('dark')).assertFalse();

      expect(migrateLegacyAppearanceMode('graphite')).assertEqual('dark');
      expect(migrateLegacyAppearanceMode('oled')).assertEqual('dark');
      expect(migrateLegacyAppearanceMode('OLED')).assertEqual('dark');
      expect(orbitNormalizeThemeMode('light')).assertEqual('light');
      expect(orbitNormalizeThemeMode('dark')).assertEqual('dark');
      expect(orbitNormalizeThemeMode('system')).assertEqual('system');
      expect(orbitNormalizeThemeMode('corrupted_value')).assertEqual('system');

      expect(resolveOrbitAppearanceIndex('system')).assertEqual(0);
      expect(resolveOrbitAppearanceIndex('light')).assertEqual(1);
      expect(resolveOrbitAppearanceIndex('dark')).assertEqual(2);
      expect(resolveOrbitAppearanceIndex('graphite')).assertEqual(2);
      expect(resolveOrbitAppearanceIndex('oled')).assertEqual(2);
    });

    it('supports trilingual EN / zh_CN / ja_JP settings search with empty-state and host filtering', 0, () => {
      // English
      const enSandbox = searchOrbitSettings('sandbox');
      expect(enSandbox.length > 0).assertTrue();
      expect(enSandbox[0].entry.destination).assertEqual('sandbox');

      const enPair = searchOrbitSettings('pairing qr');
      expect(enPair.length > 0).assertTrue();
      expect(
        enPair.some((h: OrbitSettingsSearchHit): boolean =>
          h.entry.destination === 'hosts' ||
          h.entry.destination === 'host_detail' ||
          h.entry.destination === 'pair_host'
        )
      ).assertTrue();

      // Simplified Chinese
      const zhMemory = searchOrbitSettings('项目记忆');
      expect(zhMemory.length > 0).assertTrue();
      expect(zhMemory[0].entry.destination).assertEqual('memory');

      const zhLang = searchOrbitSettings('语言');
      expect(zhLang.length > 0).assertTrue();
      expect(zhLang[0].entry.destination).assertEqual('general');

      // Japanese
      const jaPlugins = searchOrbitSettings('プラグイン');
      expect(jaPlugins.length > 0).assertTrue();
      expect(jaPlugins[0].entry.destination).assertEqual('plugins');

      const jaUsage = searchOrbitSettings('クォータ');
      expect(jaUsage.length > 0).assertTrue();
      expect(jaUsage[0].entry.destination).assertEqual('usage_quotas');

      // Empty & no match
      expect(searchOrbitSettings('').length).assertEqual(0);
      expect(searchOrbitSettings('   ').length).assertEqual(0);
      expect(searchOrbitSettings('xyz_nonexistent_setting_999').length).assertEqual(0);

      // Host-only filtering when no host is paired
      const noHostFiltered = searchOrbitSettings('sandbox', false, false);
      expect(noHostFiltered.length).assertEqual(0);
      const noHostPrefs = searchOrbitSettings('appearance', false, false);
      expect(noHostPrefs.length > 0).assertTrue();
    });

    it('blocks cross-host, ABA, scope-change, and post-unmount late replies via OrbitSettingsRequestGuard', 0, async () => {
      const guard = new OrbitSettingsRequestGuard();
      const bridge = new FakePr6CoreBridge();
      const store = createPr6TestStore(bridge);

      const d1 = createDeferredString();
      const d2 = createDeferredString();
      bridge.usageDeferredByHost.set('h1', d1);
      bridge.usageDeferredByHost.set('h2', d2);

      let renderedHost = '';
      let renderedCost = -1;
      let toastCount = 0;

      // Launch h1 request
      const tok1 = guard.begin('h1', 'usage_7');
      const p1 = store.getUsageInsightsScoped('h1', 7).then((res: OrbitScopedActionOutcome<UsageInsightsSnapshotDto>) => {
        if (!guard.isCurrent(tok1)) {
          return;
        }
        if (res.ok && res.data !== null) {
          renderedHost = 'h1';
          renderedCost = res.data.total_cost_usd;
          toastCount += 1;
        }
      });

      // Switch to h2 BEFORE h1 replies
      const tok2 = guard.begin('h2', 'usage_7');
      const p2 = store.getUsageInsightsScoped('h2', 7).then((res: OrbitScopedActionOutcome<UsageInsightsSnapshotDto>) => {
        if (!guard.isCurrent(tok2)) {
          return;
        }
        if (res.ok && res.data !== null) {
          renderedHost = 'h2';
          renderedCost = res.data.total_cost_usd;
          toastCount += 1;
        }
      });

      // h2 settles first
      d2.resolve('{"days":7,"total_cost_usd":9.99,"total_cache_savings_usd":0,"total_input_tokens":1,"total_output_tokens":1,"total_cache_read_tokens":0,"total_turns":1,"daily":[],"by_model":[],"by_project":[],"forecasts":[]}');
      await p2;
      expect(renderedHost).assertEqual('h2');
      expect(renderedCost).assertEqual(9.99);
      expect(toastCount).assertEqual(1);

      // Late h1 reply arrives after switch -> MUST be ignored!
      d1.resolve('{"days":7,"total_cost_usd":1.11,"total_cache_savings_usd":0,"total_input_tokens":1,"total_output_tokens":1,"total_cache_read_tokens":0,"total_turns":1,"daily":[],"by_model":[],"by_project":[],"forecasts":[]}');
      await p1;
      expect(renderedHost).assertEqual('h2');
      expect(renderedCost).assertEqual(9.99);
      expect(toastCount).assertEqual(1);

      // ABA switch: h2 -> h1 -> h2 invalidates prior h2 token
      const tokA = guard.begin('h2', 'usage_7');
      guard.invalidate('h1', 'usage_7');
      guard.invalidate('h2', 'usage_7');
      expect(guard.isCurrent(tokA)).assertFalse();

      // Unmount invalidates live token immediately
      const tokLive = guard.begin('h2', 'usage_30');
      expect(guard.isCurrent(tokLive)).assertTrue();
      guard.unmount();
      expect(guard.isCurrent(tokLive)).assertFalse();
    });

    it('reliably resets pull-to-refresh state on success, error, scope switch, and unmount', 0, async () => {
      const ctrl = new OrbitPullRefreshController();
      const states: boolean[] = [];

      // 1. Success path
      const ok = await ctrl.run(
        'h1::ag1',
        async (): Promise<void> => {
          await Promise.resolve();
        },
        (r: boolean): void => {
          states.push(r);
        }
      );
      expect(ok).assertTrue();
      expect(ctrl.isRefreshing).assertFalse();
      expect(states.length).assertEqual(2);
      expect(states[0]).assertTrue();
      expect(states[1]).assertFalse();

      // 2. Error / rejection path MUST reset refreshing -> false
      const errStates: boolean[] = [];
      const failed = await ctrl.run(
        'h1::ag1',
        async (): Promise<void> => {
          throw new Error('network timeout');
        },
        (r: boolean): void => {
          errStates.push(r);
        }
      );
      expect(failed).assertFalse();
      expect(ctrl.isRefreshing).assertFalse();
      expect(errStates.length).assertEqual(2);
      expect(errStates[0]).assertTrue();
      expect(errStates[1]).assertFalse();

      // 3. Scope switch during in-flight refresh immediately resets and ignores late settle
      const deferred = createDeferredString();
      let liveFlag = false;
      const inflight = ctrl.run(
        'h1::ag1',
        async (): Promise<void> => {
          await deferred.promise;
        },
        (r: boolean): void => {
          liveFlag = r;
        }
      );
      expect(liveFlag).assertTrue();
      expect(ctrl.isRefreshing).assertTrue();
      const resetOccurred = ctrl.onScopeChanged('h2::ag2');
      expect(resetOccurred).assertTrue();
      expect(ctrl.isRefreshing).assertFalse();
      liveFlag = false;
      deferred.resolve('done');
      await inflight;
      expect(ctrl.isRefreshing).assertFalse();
      expect(liveFlag).assertFalse();

      // 4. Unmount clears refreshing state
      const tok = ctrl.begin('h2::ag2');
      expect(tok > 0).assertTrue();
      expect(ctrl.isRefreshing).assertTrue();
      ctrl.unmount();
      expect(ctrl.isRefreshing).assertFalse();
    });

    it('maps four-tier native haptics accurately and falls back to 10ms time vibration on unsupported or errors', 0, async () => {
      // Exact SDK preset IDs
      expect(resolveNativeHapticEffectId('selection')).assertEqual('haptic.effect.soft');
      expect(resolveNativeHapticEffectId('action')).assertEqual('haptic.effect.soft');
      expect(resolveNativeHapticEffectId('success')).assertEqual('haptic.notice.success');
      expect(resolveNativeHapticEffectId('warning')).assertEqual('haptic.notice.warning');

      // Backward-compatible numeric normalization
      expect(normalizeOrbitHapticPreset(6)).assertEqual('selection');
      expect(normalizeOrbitHapticPreset(10)).assertEqual('action');
      expect(normalizeOrbitHapticPreset(14)).assertEqual('success');
      expect(normalizeOrbitHapticPreset(18)).assertEqual('warning');

      // 1. Supported presets fire preset vibration without fallback
      const presetCalls: string[] = [];
      const timeCalls: number[] = [];
      const okDriver: OrbitNativeHapticDriver = {
        isAvailable: (): boolean => true,
        isSupportEffectSync: (_id: string): boolean => true,
        startPresetVibration: async (id: string): Promise<void> => {
          presetCalls.push(id);
        },
        startTimeVibration: async (ms: number): Promise<void> => {
          timeCalls.push(ms);
        }
      };
      const allPresets: OrbitHapticPreset[] = ['selection', 'action', 'success', 'warning'];
      for (let i = 0; i < allPresets.length; i++) {
        triggerNativeHapticWithDriver(allPresets[i], okDriver);
      }
      expect(presetCalls.length).assertEqual(4);
      expect(presetCalls[0]).assertEqual('haptic.effect.soft');
      expect(presetCalls[1]).assertEqual('haptic.effect.soft');
      expect(presetCalls[2]).assertEqual('haptic.notice.success');
      expect(presetCalls[3]).assertEqual('haptic.notice.warning');
      expect(timeCalls.length).assertEqual(0);

      // 2. Unsupported preset falls back to 10ms time vibration
      const unsupportedTimes: number[] = [];
      const unsupportedDriver: OrbitNativeHapticDriver = {
        isAvailable: (): boolean => true,
        isSupportEffectSync: (_id: string): boolean => false,
        startPresetVibration: async (_id: string): Promise<void> => {},
        startTimeVibration: async (ms: number): Promise<void> => {
          unsupportedTimes.push(ms);
        }
      };
      triggerNativeHapticWithDriver('success', unsupportedDriver);
      expect(unsupportedTimes.length).assertEqual(1);
      expect(unsupportedTimes[0]).assertEqual(10);

      // 3. Probe throwing synchronously falls back to 10ms time vibration
      const throwProbeTimes: number[] = [];
      const throwProbeDriver: OrbitNativeHapticDriver = {
        isAvailable: (): boolean => true,
        isSupportEffectSync: (_id: string): boolean => {
          throw new Error('sensor error');
        },
        startPresetVibration: async (_id: string): Promise<void> => {},
        startTimeVibration: async (ms: number): Promise<void> => {
          throwProbeTimes.push(ms);
        }
      };
      triggerNativeHapticWithDriver('warning', throwProbeDriver);
      expect(throwProbeTimes.length).assertEqual(1);
      expect(throwProbeTimes[0]).assertEqual(10);

      // 4. Async preset rejection falls back to 10ms time vibration
      const rejectTimes: number[] = [];
      const rejectDriver: OrbitNativeHapticDriver = {
        isAvailable: (): boolean => true,
        isSupportEffectSync: (_id: string): boolean => true,
        startPresetVibration: async (_id: string): Promise<void> => {
          throw new Error('preset failed');
        },
        startTimeVibration: async (ms: number): Promise<void> => {
          rejectTimes.push(ms);
        }
      };
      triggerNativeHapticWithDriver('action', rejectDriver);
      await Promise.resolve();
      await Promise.resolve();
      expect(rejectTimes.length).assertEqual(1);
      expect(rejectTimes[0]).assertEqual(10);
    });

    it('zeros animation durations under reducedMotion and normalizes language and send behavior', 0, () => {
      expect(OrbitTheme.resolveDuration(180, true)).assertEqual(0);
      expect(OrbitTheme.resolveDuration(180, false)).assertEqual(180);
      expect(OrbitTheme.resolveMotion(OrbitTheme.MOTION_UI, true).duration).assertEqual(0);
      expect(OrbitTheme.resolveMotion(OrbitTheme.MOTION_UI, false).duration).assertEqual(OrbitTokens.DUR_UI);

      expect(normalizeOrbitAppLanguage('en')).assertEqual('en');
      expect(normalizeOrbitAppLanguage('zh_CN')).assertEqual('zh_CN');
      expect(normalizeOrbitAppLanguage('ja_JP')).assertEqual('ja_JP');
      expect(normalizeOrbitAppLanguage('zh-Hans')).assertEqual('zh_CN');
      expect(normalizeOrbitAppLanguage('ja-JP')).assertEqual('ja_JP');
      expect(normalizeOrbitAppLanguage('unknown')).assertEqual('system');

      const bridge = new FakePr6CoreBridge();
      const store = createPr6TestStore(bridge);
      expect(store.activeTurnSendBehavior).assertEqual('queue');
      store.setActiveTurnSendBehavior('steer');
      expect(store.activeTurnSendBehavior).assertEqual('steer');
      store.setActiveTurnSendBehavior('interrupt');
      expect(store.activeTurnSendBehavior).assertEqual('interrupt');
      store.setActiveTurnSendBehavior('bogus');
      expect(store.activeTurnSendBehavior).assertEqual('queue');
    });

    it('preserves literal trailing-space cwd across MCP, Skills, and Memory scoped RPCs (P0)', 0, async () => {
      const bridge = new FakePr6CoreBridge();
      const store = createPr6TestStore(bridge);
      const trailingCwd = '/repo/project ';

      await store.listExternalMcpServersScoped('h1', trailingCwd);
      await store.toggleExternalMcpServerScoped('h1', 'orbit_user:fs', false, trailingCwd);
      await store.listManagedSkillsScoped('h1', trailingCwd);
      await store.listProjectMemoriesScoped('h1', trailingCwd, 50);
      await store.confirmProjectMemoryScoped('h1', 'mem-space', trailingCwd);
      await store.deleteProjectMemoryScoped('h1', 'mem-space', trailingCwd);

      expect(bridge.listMcpCalls[0]).assertEqual(`h1|${trailingCwd}`);
      expect(bridge.toggleMcpCalls[0]).assertEqual(`h1|orbit_user:fs|0|${trailingCwd}`);
      expect(bridge.listSkillsCalls[0]).assertEqual(`h1|${trailingCwd}`);
      expect(bridge.listMemoriesCalls[0]).assertEqual(`h1|${trailingCwd}|50`);
      expect(bridge.confirmMemoryCalls[0]).assertEqual(`h1|mem-space|${trailingCwd}`);
      expect(bridge.deleteMemoryCalls[0]).assertEqual(`h1|mem-space|${trailingCwd}`);
    });

    it('blocks stale Memory delete confirmations and global hostSelectionEpoch ABA switches (P1-1 & P1-2)', 0, async () => {
      const guard = new OrbitSettingsRequestGuard();
      const bridge = new FakePr6CoreBridge();
      bridge.fleetJson = '{"revision":1,"hosts":[{"host_id":"h1","display_name":"Host 1","status":"connected"},{"host_id":"h2","display_name":"Host 2","status":"connected"}],"projects":[],"total_pending_approvals":0}';
      const store = createPr6TestStore(bridge);
      store.selectHost('h1');
      const epoch1 = store.hostSelectionEpoch;

      // Freeze delete confirmation on h1
      const snap = guard.freezeDeleteConfirm('h1', 'memory::/repo ', '/repo ', 'item', epoch1, 1);
      expect(guard.isDeleteConfirmValid(snap, 'h1', 'memory::/repo ', '/repo ', epoch1, 1)).assertTrue();

      // Switch global host h1 -> h2 and back h2 -> h1 (ABA)
      store.selectHost('h2');
      const epoch2 = store.hostSelectionEpoch;
      expect(epoch2 > epoch1).assertTrue();
      expect(guard.isDeleteConfirmValid(snap, 'h1', 'memory::/repo ', '/repo ', epoch2, 1)).assertFalse();

      store.selectHost('h1');
      const epoch3 = store.hostSelectionEpoch;
      expect(epoch3 > epoch2).assertTrue();
      expect(guard.isDeleteConfirmValid(snap, 'h1', 'memory::/repo ', '/repo ', epoch3, 1)).assertFalse();

      // New snapshot invalidated by cwd change, cancel, hide, and unmount
      const snap2 = guard.freezeDeleteConfirm('h1', 'memory::/repo', '/repo', 'item', epoch3, 2);
      expect(guard.isDeleteConfirmValid(snap2, 'h1', 'memory::/repo ', '/repo ', epoch3, 2)).assertFalse();
      guard.cancelDeleteConfirm();
      expect(guard.isDeleteConfirmValid(snap2, 'h1', 'memory::/repo', '/repo', epoch3, 2)).assertFalse();

      const snap3 = guard.freezeDeleteConfirm('h1', 'memory::/repo', '/repo', 'item', epoch3, 2);
      guard.setVisible(false);
      expect(guard.isDeleteConfirmValid(snap3, 'h1', 'memory::/repo', '/repo', epoch3, 2)).assertFalse();

      // Test Push scoped helper returns structured outcome without polluting global toast
      store.toastMessage = '';
      store.toastResource = null;
      const pushOk = await store.sendTestPushNotificationScoped('h1');
      expect(pushOk.ok).assertTrue();
      expect(bridge.testPushCalls.length).assertEqual(1);
      expect(store.toastMessage).assertEqual('');
      expect(store.toastResource === null).assertTrue();

      // copyToClipboard uses localized $r('app.string.*') resource rather than bare English toastMessage
      store.copyToClipboard('orbit://pair/demo-token');
      expect(store.toastResource !== null).assertTrue();

      // Host-scoped extension cwd NEVER leaks h1's activeCwd when targeting h2
      expect(
        resolveHostScopedExtensionCwdPure('h2', 'h1', '/host1-only', [], true, '')
      ).assertEqual(null);
      expect(
        resolveHostScopedExtensionCwdPure('h2', 'h1', '/host1-only', ['/host2-repo '], true, '')
      ).assertEqual('/host2-repo ');
      expect(
        resolveHostScopedExtensionCwdPure('h1', 'h1', '/host1-only ', ['/host1-only '], true, '')
      ).assertEqual('/host1-only ');
    });

    it('isolates same-page concurrent row writes so settling B never orphans A (P1-3)', 0, () => {
      const guard = new OrbitSettingsRequestGuard();
      const loadTok = guard.beginLoad('h1', 'mcp::/repo', 1, 1);
      const tokA = guard.beginItemWrite('h1', 'mcp::/repo', 'server-A', 1, 1);
      const tokB = guard.beginItemWrite('h1', 'mcp::/repo', 'server-B', 1, 1);

      // B completes first; BOTH B and A remain current, and list load remains unaffected
      expect(guard.isItemWriteCurrent(tokB, 1, 1)).assertTrue();
      expect(guard.isItemWriteCurrent(tokA, 1, 1)).assertTrue();
      expect(guard.isCurrent(loadTok, 1, 1)).assertTrue();

      // Switching scope immediately invalidates both A and B
      guard.beginLoad('h1', 'mcp::/other', 1, 1);
      expect(guard.isItemWriteCurrent(tokA, 1, 1)).assertFalse();
      expect(guard.isItemWriteCurrent(tokB, 1, 1)).assertFalse();
    });

    it('matches all rendered trilingual UI titles, direct host_detail search, and system locale display (P1-4 & P1-7)', 0, () => {
      // All 5 previously missed zh_CN rendered titles
      const zhTargets: Array<[string, string]> = [
        ['主机管理', 'hosts'],
        ['个人代理与互联', 'personal_agent'],
        ['代理与模型', 'agents_models'],
        ['权限与 Guardian', 'permissions_guardian'],
        ['用量与配额', 'usage_quotas']
      ];
      for (let i = 0; i < zhTargets.length; i++) {
        const hits = searchOrbitSettings(zhTargets[i][0]);
        expect(hits.length > 0).assertTrue();
        expect(hits[0].entry.destination).assertEqual(zhTargets[i][1]);
      }

      // All 4 previously missed ja_JP rendered titles
      const jaTargets: Array<[string, string]> = [
        ['パーソナルエージェント＆メッシュ', 'personal_agent'],
        ['エージェントとモデル', 'agents_models'],
        ['権限と Guardian', 'permissions_guardian'],
        ['使用量とクォータ', 'usage_quotas']
      ];
      for (let i = 0; i < jaTargets.length; i++) {
        const hits = searchOrbitSettings(jaTargets[i][0]);
        expect(hits.length > 0).assertTrue();
        expect(hits[0].entry.destination).assertEqual(jaTargets[i][1]);
      }

      // Direct host_detail search destination
      const detailHits = searchOrbitSettings('host detail');
      expect(detailHits.length > 0).assertTrue();
      expect(detailHits[0].entry.destination).assertEqual('host_detail');
      const zhDetailHits = searchOrbitSettings('主机详情');
      expect(zhDetailHits.length > 0).assertTrue();
      expect(zhDetailHits[0].entry.destination).assertEqual('host_detail');

      // System language preference resolves effective locale instead of defaulting to English
      expect(resolveEffectiveOrbitLanguage('system', 'zh-Hans')).assertEqual('zh_CN');
      expect(resolveEffectiveOrbitLanguage('system', 'ja-JP')).assertEqual('ja_JP');
      expect(resolveEffectiveOrbitLanguage('system', 'en-US')).assertEqual('en');
      const hostsEntry = searchOrbitSettings('hosts')[0].entry;
      expect(localizeSettingsSearchTitle(hostsEntry, 'system', 'zh-Hans')).assertEqual('主机管理');
      expect(localizeSettingsSearchGroup(hostsEntry, 'system', 'ja-JP')).assertEqual('フリートと同期');

      // Default Model and Effort setters update store selection (P1-7)
      const bridge = new FakePr6CoreBridge();
      const store = createPr6TestStore(bridge);
      store.setComposerModel('gpt-5.4');
      expect(store.composerModel).assertEqual('gpt-5.4');
      store.setComposerEffort('high');
      expect(store.composerEffort).assertEqual('high');
    });

    it('persists host-scoped default provider, model, and effort across cold store rebuild without touching active agents (R2-P1-3 & R2-P1-4)', 0, () => {
      // Shared disk-backed Preferences simulation across Store instances
      const prefsBackend = new InMemoryPr6PrefsBackend();
      prefsBackend.backing.set(
        'orbit.harmony.v2.draft_prefs',
        'provider=codex\nmodel=m1\neffort=low\npermissionMode=local\nsidebarProjectSort=recent\nsidebarThreadFilter=all'
      );

      const h1Detail = '{"host":{"host_id":"h1","display_name":"Host 1","status":"connected","available_providers":["codex","claude_code"]},"agents":[],"provider_catalogs":[{"provider":"codex","models":[{"id":"m1","label":"M1","description":"","is_default":true,"default_effort":"low","thinking_options":[{"effort":"low","label":"Low","is_default":true},{"effort":"high","label":"High","is_default":false}]},{"id":"m2","label":"M2","description":"","is_default":false,"default_effort":"high","thinking_options":[{"effort":"low","label":"Low","is_default":false},{"effort":"high","label":"High","is_default":true}]}],"configured_default_model":"m1","configured_default_effort":"low"}],"quotas":[],"discovered_projects":[],"workspaces":[]}';
      const h2Detail = '{"host":{"host_id":"h2","display_name":"Host 2","status":"connected","available_providers":["claude_code"]},"agents":[],"provider_catalogs":[{"provider":"claude_code","models":[{"id":"claude-sonnet-4-6","label":"Sonnet","description":"","is_default":true,"default_effort":"medium","thinking_options":[{"effort":"medium","label":"Medium","is_default":true},{"effort":"high","label":"High","is_default":false}]}],"configured_default_model":"claude-sonnet-4-6","configured_default_effort":"medium"}],"quotas":[],"discovered_projects":[],"workspaces":[]}';
      const fleet = '{"revision":1,"hosts":[{"host_id":"h1","display_name":"Host 1","status":"connected"},{"host_id":"h2","display_name":"Host 2","status":"connected"}],"projects":[],"total_pending_approvals":0}';

      // 1. Store #1 restores m1/low from pre-seeded preferences in new-session draft mode
      const bridge1 = new FakePr6CoreBridge();
      bridge1.fleetJson = fleet;
      bridge1.hostDetailByHost.set('h1', h1Detail);
      bridge1.hostDetailByHost.set('h2', h2Detail);
      const store1 = new OrbitHarmonyStore();
      store1.bindPreferencesBackendForTest(prefsBackend);
      store1.initializeWithTestBridge(bridge1);
      store1.selectHost('h1');
      store1.isNewSessionDraft = true;
      store1.selectedAgentId = '';
      expect(store1.composerProvider).assertEqual('codex');
      expect(store1.composerModel).assertEqual('m1');
      expect(store1.composerEffort).assertEqual('low');

      // 2. Select m2 & high via host-scoped Settings defaults -> MUST write & flush Preferences
      const putsBefore = prefsBackend.putCount;
      const flushesBefore = prefsBackend.flushCount;
      store1.setHostDefaultModel('h1', 'm2');
      store1.setHostDefaultEffort('h1', 'high');
      expect(store1.composerModel).assertEqual('m2');
      expect(store1.composerEffort).assertEqual('high');
      expect(prefsBackend.putCount > putsBefore).assertTrue();
      expect(prefsBackend.flushCount > flushesBefore).assertTrue();

      // 3. Cold-start Store #2 from the SAME backing store -> MUST restore m2/high!
      const bridge2 = new FakePr6CoreBridge();
      bridge2.fleetJson = fleet;
      bridge2.hostDetailByHost.set('h1', h1Detail);
      bridge2.hostDetailByHost.set('h2', h2Detail);
      const store2 = new OrbitHarmonyStore();
      store2.bindPreferencesBackendForTest(prefsBackend);
      store2.initializeWithTestBridge(bridge2);
      store2.selectHost('h1');
      expect(store2.getHostDefaultModel('h1')).assertEqual('m2');
      expect(store2.getHostDefaultEffort('h1')).assertEqual('high');
      expect(store2.composerModel).assertEqual('m2');
      expect(store2.composerEffort).assertEqual('high');

      // 4. R2-P1-4: With h1 running `agent-from-h1`, setting defaults on h2 MUST emit 0 switchAgentProvider / setAgentConfig RPCs!
      store2.isNewSessionDraft = false;
      store2.selectedAgentId = 'agent-from-h1';
      store2.composerProvider = 'codex';
      store2.composerModel = 'm2';
      store2.composerEffort = 'high';
      store2.setHostDefaultProvider('h2', 'claude_code');
      store2.setHostDefaultModel('h2', 'claude-sonnet-4-6');
      store2.setHostDefaultEffort('h2', 'high');

      expect(bridge2.switchAgentProviderCalls.length).assertEqual(0);
      expect(bridge2.setAgentConfigCalls.length).assertEqual(0);
      expect(store2.composerProvider).assertEqual('codex');
      expect(store2.composerModel).assertEqual('m2');
      expect(store2.composerEffort).assertEqual('high');
      expect(store2.getHostDefaultProvider('h2')).assertEqual('claude_code');
      expect(store2.getHostDefaultModel('h2')).assertEqual('claude-sonnet-4-6');
      expect(store2.getHostDefaultEffort('h2')).assertEqual('high');

      // Even changing h1 defaults while h1 has a running agent MUST NOT call switchAgentProvider on the live agent
      store2.setHostDefaultProvider('h1', 'claude_code');
      expect(bridge2.switchAgentProviderCalls.length).assertEqual(0);
      expect(store2.composerProvider).assertEqual('codex');
      expect(store2.getHostDefaultProvider('h1')).assertEqual('claude_code');
    });

    it('rejects stale workspace delete confirms on live cwd mismatch and supports hide->show reload recovery (R2-P0-1 & R2-P1-2)', 0, () => {
      const guard = new OrbitSettingsRequestGuard();
      const load1 = guard.beginLoad('h1', 'h1\u0000/east', 1, 1);
      expect(guard.isCurrent(load1, 1, 1)).assertTrue();

      // Freeze delete confirm on east; switching live cwd to west MUST reject even if host matches
      const snap = guard.freezeDeleteConfirm('h1', 'h1\u0000/east', '/east', 'same', 1, 1);
      expect(guard.isDeleteConfirmValid(snap, 'h1', 'h1\u0000/east', '/east', 1, 1)).assertTrue();
      expect(guard.isDeleteConfirmValid(snap, 'h1', 'h1\u0000/west', '/west', 1, 1)).assertFalse();

      // Hide invalidates in-flight load; re-showing and launching fresh load succeeds cleanly
      const inflight = guard.beginLoad('h1', 'h1\u0000/west', 1, 1);
      guard.setVisible(false);
      expect(guard.isVisible).assertFalse();
      expect(guard.isCurrent(inflight, 1, 1)).assertFalse();
      guard.setVisible(true);
      expect(guard.isVisible).assertTrue();
      const reloaded = guard.beginLoad('h1', 'h1\u0000/west', 1, 1);
      expect(guard.isCurrent(reloaded, 1, 1)).assertTrue();
    });

    it('enforces immutable load-batch Memory row bindings for POSIX backslash and symlink workspaces plus host-scoped Provider defaults (R4-P0-1, R4-P1-1 & R3-P1-3)', 0, () => {
      const makeMem = (id: string, workspaceRoot: string, title: string): ProjectMemoryItemDto => ({
        id: id,
        workspace_root: workspaceRoot,
        title: title,
        content: 'body',
        category: 'convention',
        status: 'hypothesis',
        confidence: 0.9,
        file_paths: [],
        tags: [],
        source_provider: 'claude',
        source_agent_id: 'ag-1',
        source_path: null,
        access_count: 1,
        created_at_ms: 1,
        updated_at_ms: 1,
        last_accessed_ms: 1
      });

      // R4-P0-1: POSIX `east\` (U+005C) and `east` are strictly distinct workspaces.
      // A stale row binding captured from `east\` (`loadSeq = 1`) MUST be rejected once `east` (`loadSeq = 2`) loads,
      // even when both workspaces contain the exact same item ID (`stale`).
      const eastBackslashCwd = '/tmp/work/east\\';
      const eastPlainCwd = '/tmp/work/east';
      const eastBackslashBinding: OrbitMemoryRowBinding = {
        loadSeq: 1,
        hostId: 'h1',
        hostSelectionEpoch: 3,
        rootEpoch: 1,
        visibleEpoch: 2,
        scopeKey: `h1\u0000${eastBackslashCwd}`,
        requestCwd: eastBackslashCwd,
        item: makeMem('stale', eastBackslashCwd, 'Backslash Title')
      };
      const eastPlainBinding: OrbitMemoryRowBinding = {
        loadSeq: 2,
        hostId: 'h1',
        hostSelectionEpoch: 3,
        rootEpoch: 1,
        visibleEpoch: 2,
        scopeKey: `h1\u0000${eastPlainCwd}`,
        requestCwd: eastPlainCwd,
        item: makeMem('stale', eastPlainCwd, 'Plain East Title')
      };

      // Active while `east\` batch 1 is live
      expect(
        isMemoryRowBindingActive(
          eastBackslashBinding,
          true,
          true,
          2,
          1,
          'h1',
          3,
          1,
          `h1\u0000${eastBackslashCwd}`,
          eastBackslashCwd,
          [eastBackslashBinding]
        )
      ).assertTrue();

      // Immediately rejected during cross-scope transition (`loadedListSeq = 0`, cleared list)
      expect(
        isMemoryRowBindingActive(
          eastBackslashBinding,
          true,
          true,
          2,
          0,
          'h1',
          3,
          1,
          `h1\u0000${eastPlainCwd}`,
          eastPlainCwd,
          []
        )
      ).assertFalse();

      // Still rejected after `east` batch 2 finishes loading (0 dialog, 0 RPC!)
      expect(
        isMemoryRowBindingActive(
          eastBackslashBinding,
          true,
          true,
          2,
          2,
          'h1',
          3,
          1,
          `h1\u0000${eastPlainCwd}`,
          eastPlainCwd,
          [eastPlainBinding]
        )
      ).assertFalse();
      expect(
        isMemoryRowBindingActive(
          eastPlainBinding,
          true,
          true,
          2,
          2,
          'h1',
          3,
          1,
          `h1\u0000${eastPlainCwd}`,
          eastPlainCwd,
          [eastPlainBinding]
        )
      ).assertTrue();

      // R4-P1-1: macOS symlink alias `/tmp/demo/east/` where daemon returns canonical `/private/tmp/demo/east`
      // MUST remain actionable with zero client path string guessing and preserve verbatim `requestCwd`.
      const symlinkRequestCwd = '/tmp/demo/east/';
      const symlinkBinding: OrbitMemoryRowBinding = {
        loadSeq: 3,
        hostId: 'h1',
        hostSelectionEpoch: 3,
        rootEpoch: 1,
        visibleEpoch: 2,
        scopeKey: `h1\u0000${symlinkRequestCwd}`,
        requestCwd: symlinkRequestCwd,
        item: makeMem('sym-1', '/private/tmp/demo/east', 'Symlink Memory')
      };
      expect(
        isMemoryRowBindingActive(
          symlinkBinding,
          true,
          true,
          2,
          3,
          'h1',
          3,
          1,
          `h1\u0000${symlinkRequestCwd}`,
          symlinkRequestCwd,
          [symlinkBinding]
        )
      ).assertTrue();
      expect(symlinkBinding.requestCwd).assertEqual('/tmp/demo/east/');

      // Host epoch bump (ABA) invalidates symlink binding immediately
      expect(
        isMemoryRowBindingActive(
          symlinkBinding,
          true,
          true,
          2,
          3,
          'h1',
          4,
          1,
          `h1\u0000${symlinkRequestCwd}`,
          symlinkRequestCwd,
          [symlinkBinding]
        )
      ).assertFalse();

      // R3-P1-3: selecting Claude on h2 MUST use h2's configured_default_model (`h2-default`), NEVER h1's `only-h1`!
      const prefsBackend = new InMemoryPr6PrefsBackend();
      const fleet = '{"revision":1,"hosts":[{"host_id":"h1","display_name":"Host 1","status":"connected"},{"host_id":"h2","display_name":"Host 2","status":"connected"}],"projects":[],"total_pending_approvals":0}';
      const h1Detail = '{"host":{"host_id":"h1","display_name":"Host 1","status":"connected","available_providers":["claude_code"]},"agents":[],"provider_catalogs":[{"provider":"claude_code","models":[{"id":"only-h1","label":"Only H1","description":"","is_default":true,"default_effort":"low","thinking_options":[{"effort":"low","label":"Low","is_default":true}]}],"configured_default_model":"only-h1","configured_default_effort":"low"}],"quotas":[],"discovered_projects":[],"workspaces":[]}';
      const h2Detail = '{"host":{"host_id":"h2","display_name":"Host 2","status":"connected","available_providers":["claude_code"]},"agents":[],"provider_catalogs":[{"provider":"claude_code","models":[{"id":"h2-default","label":"H2 Default","description":"","is_default":true,"default_effort":"medium","thinking_options":[{"effort":"medium","label":"Medium","is_default":true},{"effort":"high","label":"High","is_default":false}]},{"id":"h2-alt","label":"H2 Alt","description":"","is_default":false,"default_effort":"high","thinking_options":[{"effort":"high","label":"High","is_default":true}]}],"configured_default_model":"h2-default","configured_default_effort":"medium"}],"quotas":[],"discovered_projects":[],"workspaces":[]}';

      const bridge = new FakePr6CoreBridge();
      bridge.fleetJson = fleet;
      bridge.hostDetailByHost.set('h1', h1Detail);
      bridge.hostDetailByHost.set('h2', h2Detail);
      const store = new OrbitHarmonyStore();
      store.bindPreferencesBackendForTest(prefsBackend);
      store.initializeWithTestBridge(bridge);
      store.selectHost('h1');
      store.reloadHost('h2');
      store.isNewSessionDraft = false;
      store.selectedAgentId = 'agent-from-h1';
      store.composerProvider = 'claude_code';
      store.composerModel = 'only-h1';
      store.composerEffort = 'low';

      // Click Claude Provider on h2 settings page
      store.setHostDefaultProvider('h2', 'claude_code');

      expect(store.getHostDefaultProvider('h2')).assertEqual('claude_code');
      expect(store.getHostDefaultModel('h2')).assertEqual('h2-default');
      expect(store.getHostDefaultEffort('h2')).assertEqual('medium');
      expect(bridge.switchAgentProviderCalls.length).assertEqual(0);
      expect(bridge.setAgentConfigCalls.length).assertEqual(0);
      expect(store.composerModel).assertEqual('only-h1');

      // Persisted JSON on disk for h2 MUST contain h2-default and NEVER only-h1
      const persistedMapRaw = prefsBackend.backing.get('orbit.harmony.v2.host_default_agent_prefs_json') ?? '';
      expect(persistedMapRaw).assertContain('h2-default');
      expect(persistedMapRaw.includes('only-h1')).assertFalse();
    });

    it('settles in-flight Memory confirm and delete across same-scope refresh snapshots while preserving strict pre-dispatch batch checks (R5-P1-1)', 0, () => {
      const makeMem = (id: string, title: string, status: string): ProjectMemoryItemDto => ({
        id: id,
        workspace_root: '/repo/east',
        title: title,
        content: 'body',
        category: 'convention',
        status: status,
        confidence: 0.9,
        file_paths: [],
        tags: [],
        source_provider: 'claude',
        source_agent_id: 'ag-1',
        source_path: null,
        access_count: 1,
        created_at_ms: 1,
        updated_at_ms: 1,
        last_accessed_ms: 1
      });
      const scopeKey = 'h1\u0000/repo/east';
      const guard = new OrbitSettingsRequestGuard();
      guard.beginLoad('h1', scopeKey, 1, 0);

      // Batch 8: initial list [m1(hypothesis), m2(hypothesis)]
      const b8m1: OrbitMemoryRowBinding = {
        loadSeq: 8,
        hostId: 'h1',
        hostSelectionEpoch: 1,
        rootEpoch: 0,
        visibleEpoch: 2,
        scopeKey: scopeKey,
        requestCwd: '/repo/east',
        item: makeMem('m1', 'Mem 1', 'hypothesis')
      };
      const b8m2: OrbitMemoryRowBinding = {
        loadSeq: 8,
        hostId: 'h1',
        hostSelectionEpoch: 1,
        rootEpoch: 0,
        visibleEpoch: 2,
        scopeKey: scopeKey,
        requestCwd: '/repo/east',
        item: makeMem('m2', 'Mem 2', 'hypothesis')
      };

      // 1. Confirm m1 dispatched at loadSeq=8 (held in flight); same-scope refresh arrives FIRST -> loadSeq=9 with stale hypothesis
      const confirmTok = guard.beginItemWrite('h1', scopeKey, 'm1', 1, 0);
      const refreshTok = guard.beginLoad('h1', scopeKey, 1, 0);
      expect(guard.isCurrent(refreshTok, 1, 0)).assertTrue();
      let liveBindings: OrbitMemoryRowBinding[] = [
        {
          loadSeq: 9,
          hostId: 'h1',
          hostSelectionEpoch: 1,
          rootEpoch: 0,
          visibleEpoch: 2,
          scopeKey: scopeKey,
          requestCwd: '/repo/east',
          item: makeMem('m1', 'Mem 1', 'hypothesis')
        },
        {
          loadSeq: 9,
          hostId: 'h1',
          hostSelectionEpoch: 1,
          rootEpoch: 0,
          visibleEpoch: 2,
          scopeKey: scopeKey,
          requestCwd: '/repo/east',
          item: makeMem('m2', 'Mem 2', 'hypothesis')
        }
      ];

      // Pre-dispatch old batch-8 click MUST STILL be rejected once batch-9 is live
      expect(
        isMemoryRowBindingActive(b8m1, true, true, 2, 9, 'h1', 1, 0, scopeKey, '/repo/east', liveBindings)
      ).assertFalse();
      // Yet the ALREADY-DISPATCHED confirm RPC MUST settle cleanly onto batch-9!
      expect(guard.isItemWriteCurrent(confirmTok, 1, 0)).assertTrue();
      expect(
        canSettleMemoryRowWrite(
          b8m1,
          true,
          true,
          2,
          9,
          'h1',
          scopeKey,
          '/repo/east',
          'h1',
          1,
          0,
          scopeKey,
          '/repo/east'
        )
      ).assertTrue();
      liveBindings = applySettledMemoryConfirm(liveBindings, 'm1', 999);
      expect(liveBindings.length).assertEqual(2);
      expect(liveBindings[0].item.status).assertEqual('confirmed');
      expect(liveBindings[0].loadSeq).assertEqual(9);
      expect(liveBindings[1].item.status).assertEqual('hypothesis');

      // 2. Delete m2 dispatched at loadSeq=9 (held in flight); same-scope refresh arrives FIRST -> loadSeq=10 with stale m2 still present
      const delTok = guard.beginItemWrite('h1', scopeKey, 'm2', 1, 0);
      const b9m2 = liveBindings[1];
      guard.beginLoad('h1', scopeKey, 1, 0);
      let liveBindings10: OrbitMemoryRowBinding[] = [
        {
          loadSeq: 10,
          hostId: 'h1',
          hostSelectionEpoch: 1,
          rootEpoch: 0,
          visibleEpoch: 2,
          scopeKey: scopeKey,
          requestCwd: '/repo/east',
          item: makeMem('m1', 'Mem 1', 'confirmed')
        },
        {
          loadSeq: 10,
          hostId: 'h1',
          hostSelectionEpoch: 1,
          rootEpoch: 0,
          visibleEpoch: 2,
          scopeKey: scopeKey,
          requestCwd: '/repo/east',
          item: makeMem('m2', 'Mem 2', 'hypothesis')
        }
      ];
      expect(guard.isItemWriteCurrent(delTok, 1, 0)).assertTrue();
      expect(
        canSettleMemoryRowWrite(
          b9m2,
          true,
          true,
          2,
          10,
          'h1',
          scopeKey,
          '/repo/east',
          'h1',
          1,
          0,
          scopeKey,
          '/repo/east'
        )
      ).assertTrue();
      liveBindings10 = applySettledMemoryDelete(liveBindings10, 'm2');
      expect(liveBindings10.length).assertEqual(1);
      expect(liveBindings10[0].item.id).assertEqual('m1');
      expect(liveBindings10[0].loadSeq).assertEqual(10);

      // 3. ABA cross-workspace switch (east -> west -> east) STILL rejects in-flight write!
      const abaTok = guard.beginItemWrite('h1', scopeKey, 'm1', 1, 0);
      guard.beginLoad('h1', 'h1\u0000/repo/west', 1, 0);
      guard.beginLoad('h1', scopeKey, 1, 0);
      expect(guard.isItemWriteCurrent(abaTok, 1, 0)).assertFalse();

      // 4. Same-scope refresh settlement parity for MCP and Plugins
      const sampleMcp: ExternalMcpServerRecordDto = {
        id: 'orbit_user:fs',
        name: 'filesystem',
        scope: 'orbit_user',
        transport: { kind: 'stdio', command: 'npx', args: ['-y', 'fs'] },
        env_keys: [],
        masked_env: {},
        masked_headers: {},
        enabled: true,
        editable: true,
        source_path: '~/.orbit/mcp.json',
        last_probe: null
      };
      const mcp8: OrbitMcpRowBinding = {
        loadSeq: 8,
        hostId: 'h1',
        hostSelectionEpoch: 1,
        rootEpoch: 0,
        visibleEpoch: 2,
        scopeKey: '__host__',
        requestCwd: null,
        server: sampleMcp
      };
      const mcp9: OrbitMcpRowBinding = {
        loadSeq: 9,
        hostId: 'h1',
        hostSelectionEpoch: 1,
        rootEpoch: 0,
        visibleEpoch: 2,
        scopeKey: '__host__',
        requestCwd: null,
        server: sampleMcp
      };
      expect(
        canSettleMcpRowWrite(mcp8, true, true, 2, 9, 'h1', '__host__', 'h1', 1, 0, '__host__')
      ).assertTrue();
      const mcpSettled = applySettledMcpToggle([mcp9], 'orbit_user:fs', false);
      expect(mcpSettled[0].server.enabled).assertFalse();
      expect(mcpSettled[0].loadSeq).assertEqual(9);

      const samplePlugin: InstalledPluginDto = {
        manifest: {
          id: 'orbit-lint',
          name: 'Orbit Lint',
          version: '1.0.0',
          description: 'Lint',
          author: 'Orbit',
          wasm_entry: 'plugin.wasm',
          capabilities: { fs_read_globs: [], http_domains: [], hook_events: [] },
          mcp_ops: [],
          ui_slots: []
        },
        enabled: true
      };
      const pl8: OrbitPluginRowBinding = {
        loadSeq: 8,
        hostId: 'h1',
        hostSelectionEpoch: 1,
        rootEpoch: 0,
        visibleEpoch: 2,
        plugin: samplePlugin
      };
      const pl9: OrbitPluginRowBinding = {
        loadSeq: 9,
        hostId: 'h1',
        hostSelectionEpoch: 1,
        rootEpoch: 0,
        visibleEpoch: 2,
        plugin: samplePlugin
      };
      expect(canSettlePluginRowWrite(pl8, true, true, 2, 9, 'h1', 'h1', 1, 0)).assertTrue();
      const plSettled = applySettledPluginToggle([pl9], 'orbit-lint', false);
      expect(plSettled[0].plugin.enabled).assertFalse();
      expect(plSettled[0].loadSeq).assertEqual(9);
      expect(b8m2.item.id).assertEqual('m2');
    });

    it('blocks stale Memory, MCP, Plugin, and Test Push closures after onHidden, aboutToDisappear, h1->h2 (same ID), and h1->h2->h1 ABA (R5-P1-2 & R5-P1-3)', 0, () => {
      const bridge = new FakePr6CoreBridge();
      bridge.fleetJson = '{"revision":1,"hosts":[{"host_id":"h1","display_name":"Host 1","status":"connected"},{"host_id":"h2","display_name":"Host 2","status":"connected"}],"projects":[],"total_pending_approvals":0}';
      const store = createPr6TestStore(bridge);
      store.selectHost('h1');
      const ep1 = store.hostSelectionEpoch;

      const memItem: ProjectMemoryItemDto = {
        id: 'shared-mem',
        workspace_root: '/repo',
        title: 'H1 Mem',
        content: 'body',
        category: 'convention',
        status: 'hypothesis',
        confidence: 0.9,
        file_paths: [],
        tags: [],
        source_provider: 'claude',
        source_agent_id: 'ag-1',
        source_path: null,
        access_count: 1,
        created_at_ms: 1,
        updated_at_ms: 1,
        last_accessed_ms: 1
      };
      const mcpItem: ExternalMcpServerRecordDto = {
        id: 'orbit_user:fs',
        name: 'filesystem',
        scope: 'orbit_user',
        transport: { kind: 'stdio', command: 'npx', args: ['-y', 'fs'] },
        env_keys: [],
        masked_env: {},
        masked_headers: {},
        enabled: true,
        editable: true,
        source_path: '~/.orbit/mcp.json',
        last_probe: null
      };
      const pluginItem: InstalledPluginDto = {
        manifest: {
          id: 'orbit-lint',
          name: 'Orbit Lint',
          version: '1.0.0',
          description: 'Lint',
          author: 'Orbit',
          wasm_entry: 'plugin.wasm',
          capabilities: { fs_read_globs: [], http_domains: [], hook_events: [] },
          mcp_ops: [],
          ui_slots: []
        },
        enabled: true
      };

      // Captured h1 bindings at render time (`visibleEpoch = 2`, `loadSeq = 1`, `rootEpoch = 0`)
      const h1Mem: OrbitMemoryRowBinding = {
        loadSeq: 1,
        hostId: 'h1',
        hostSelectionEpoch: ep1,
        rootEpoch: 0,
        visibleEpoch: 2,
        scopeKey: 'h1\u0000/repo',
        requestCwd: '/repo',
        item: memItem
      };
      const h1Mcp: OrbitMcpRowBinding = {
        loadSeq: 1,
        hostId: 'h1',
        hostSelectionEpoch: ep1,
        rootEpoch: 0,
        visibleEpoch: 2,
        scopeKey: '__host__',
        requestCwd: null,
        server: mcpItem
      };
      const h1Plugin: OrbitPluginRowBinding = {
        loadSeq: 1,
        hostId: 'h1',
        hostSelectionEpoch: ep1,
        rootEpoch: 0,
        visibleEpoch: 2,
        plugin: pluginItem
      };
      const h1Push = createSettingsControlBinding('h1', ep1, 0, 2, 'notifications_test');

      // Baseline active on mounted visible h1
      expect(isMemoryRowBindingActive(h1Mem, true, true, 2, 1, 'h1', ep1, 0, 'h1\u0000/repo', '/repo', [h1Mem])).assertTrue();
      expect(isMcpRowBindingActive(h1Mcp, true, true, 2, 1, 'h1', ep1, 0, '__host__', [h1Mcp])).assertTrue();
      expect(isPluginRowBindingActive(h1Plugin, true, true, 2, 1, 'h1', ep1, 0, [h1Plugin])).assertTrue();
      expect(isSettingsControlBindingActive(h1Push, true, true, 2, 'h1', ep1, 0, 'notifications_test')).assertTrue();

      // R5-P1-2A: onHidden (`pageVisible = false`, `visibleEpoch = 3`, `loadedListSeq = 0`) rejects ALL 4
      expect(isMemoryRowBindingActive(h1Mem, true, false, 3, 0, 'h1', ep1, 0, 'h1\u0000/repo', '/repo', [])).assertFalse();
      expect(isMcpRowBindingActive(h1Mcp, true, false, 3, 0, 'h1', ep1, 0, '__host__', [])).assertFalse();
      expect(isPluginRowBindingActive(h1Plugin, true, false, 3, 0, 'h1', ep1, 0, [])).assertFalse();
      expect(isSettingsControlBindingActive(h1Push, true, false, 3, 'h1', ep1, 0, 'notifications_test')).assertFalse();

      // R5-P1-2B: onShown (`pageVisible = true`, `visibleEpoch = 4`) STILL rejects pre-hide closures (both while loading AND after fresh load!)
      expect(isMemoryRowBindingActive(h1Mem, true, true, 4, 0, 'h1', ep1, 0, 'h1\u0000/repo', '/repo', [])).assertFalse();
      expect(isMcpRowBindingActive(h1Mcp, true, true, 4, 0, 'h1', ep1, 0, '__host__', [])).assertFalse();
      expect(isPluginRowBindingActive(h1Plugin, true, true, 4, 0, 'h1', ep1, 0, [])).assertFalse();
      expect(isSettingsControlBindingActive(h1Push, true, true, 4, 'h1', ep1, 0, 'notifications_test')).assertFalse();

      // R5-P1-2C: aboutToDisappear (`pageMounted = false`, `pageVisible = false`, `visibleEpoch = 5`) rejects ALL 4
      expect(isMemoryRowBindingActive(h1Mem, false, false, 5, 0, 'h1', ep1, 0, 'h1\u0000/repo', '/repo', [])).assertFalse();
      expect(isMcpRowBindingActive(h1Mcp, false, false, 5, 0, 'h1', ep1, 0, '__host__', [])).assertFalse();
      expect(isPluginRowBindingActive(h1Plugin, false, false, 5, 0, 'h1', ep1, 0, [])).assertFalse();
      expect(isSettingsControlBindingActive(h1Push, false, false, 5, 'h1', ep1, 0, 'notifications_test')).assertFalse();

      // R5-P1-3A: switch h1 -> h2 where h2 has IDENTICAL server (`orbit_user:fs`) & plugin (`orbit-lint`) IDs
      store.selectHost('h2');
      const ep2 = store.hostSelectionEpoch;
      expect(ep2 > ep1).assertTrue();
      const h2Mcp: OrbitMcpRowBinding = {
        loadSeq: 2,
        hostId: 'h2',
        hostSelectionEpoch: ep2,
        rootEpoch: 1,
        visibleEpoch: 2,
        scopeKey: '__host__',
        requestCwd: null,
        server: mcpItem
      };
      const h2Plugin: OrbitPluginRowBinding = {
        loadSeq: 2,
        hostId: 'h2',
        hostSelectionEpoch: ep2,
        rootEpoch: 1,
        visibleEpoch: 2,
        plugin: pluginItem
      };
      expect(isMcpRowBindingActive(h1Mcp, true, true, 2, 2, 'h2', ep2, 1, '__host__', [h2Mcp])).assertFalse();
      expect(isPluginRowBindingActive(h1Plugin, true, true, 2, 2, 'h2', ep2, 1, [h2Plugin])).assertFalse();
      expect(isSettingsControlBindingActive(h1Push, true, true, 2, 'h2', ep2, 1, 'notifications_test')).assertFalse();
      // Fresh h2 bindings work
      expect(isMcpRowBindingActive(h2Mcp, true, true, 2, 2, 'h2', ep2, 1, '__host__', [h2Mcp])).assertTrue();
      expect(isPluginRowBindingActive(h2Plugin, true, true, 2, 2, 'h2', ep2, 1, [h2Plugin])).assertTrue();

      // R5-P1-3B: switch h1 -> h2 -> h1 (ABA) — original h1 closures STILL rejected!
      store.selectHost('h1');
      const ep3 = store.hostSelectionEpoch;
      expect(ep3 > ep2).assertTrue();
      const h1McpFresh: OrbitMcpRowBinding = {
        loadSeq: 3,
        hostId: 'h1',
        hostSelectionEpoch: ep3,
        rootEpoch: 2,
        visibleEpoch: 2,
        scopeKey: '__host__',
        requestCwd: null,
        server: mcpItem
      };
      const h1PluginFresh: OrbitPluginRowBinding = {
        loadSeq: 3,
        hostId: 'h1',
        hostSelectionEpoch: ep3,
        rootEpoch: 2,
        visibleEpoch: 2,
        plugin: pluginItem
      };
      const h1PushFresh = createSettingsControlBinding('h1', ep3, 2, 2, 'notifications_test');
      expect(isMcpRowBindingActive(h1Mcp, true, true, 2, 3, 'h1', ep3, 2, '__host__', [h1McpFresh])).assertFalse();
      expect(isPluginRowBindingActive(h1Plugin, true, true, 2, 3, 'h1', ep3, 2, [h1PluginFresh])).assertFalse();
      expect(isSettingsControlBindingActive(h1Push, true, true, 2, 'h1', ep3, 2, 'notifications_test')).assertFalse();
      // Fresh post-ABA h1 bindings work
      expect(isMcpRowBindingActive(h1McpFresh, true, true, 2, 3, 'h1', ep3, 2, '__host__', [h1McpFresh])).assertTrue();
      expect(isPluginRowBindingActive(h1PluginFresh, true, true, 2, 3, 'h1', ep3, 2, [h1PluginFresh])).assertTrue();
      expect(isSettingsControlBindingActive(h1PushFresh, true, true, 2, 'h1', ep3, 2, 'notifications_test')).assertTrue();
    });

    it('re-selects a valid host directory after hidden host switches and stays empty on 0-directory hosts (R6-P1-1)', 0, async () => {
      const bridge = new FakePr6CoreBridge();
      const store = createPr6TestStore(bridge);

      // 1. Hidden during h1 -> h2 switch: onHostChanged clears selectedCwd -> ''; onShown picks h2's directory and loads
      let selectedCwd = '/h1/east';
      const h2Cwds = ['/h2/only '];
      // Simulate hidden host switch clearing selectedCwd
      selectedCwd = '';
      selectedCwd = resolveEffectiveMemoryCwd(selectedCwd, h2Cwds);
      expect(selectedCwd).assertEqual('/h2/only ');
      const h2Res = await store.listProjectMemoriesScoped('h2', selectedCwd, 100);
      expect(h2Res.ok).assertTrue();
      expect(bridge.listMemoriesCalls.length).assertEqual(1);
      expect(bridge.listMemoriesCalls[0]).assertEqual('h2|/h2/only |100');

      // 2. Hidden during h1 -> h2 -> h1 round trip: selectedCwd cleared while hidden; onShown restores h1's first directory and loads
      selectedCwd = '';
      const h1Cwds = ['/h1/east', '/h1/west', '/h1/east\\'];
      selectedCwd = resolveEffectiveMemoryCwd(selectedCwd, h1Cwds);
      expect(selectedCwd).assertEqual('/h1/east');
      const h1Res = await store.listProjectMemoriesScoped('h1', selectedCwd, 100);
      expect(h1Res.ok).assertTrue();
      expect(bridge.listMemoriesCalls.length).assertEqual(2);
      expect(bridge.listMemoriesCalls[1]).assertEqual('h1|/h1/east|100');

      // 3. Preserves exact byte-for-byte cwd (including trailing space/backslash) when still valid for host
      expect(resolveEffectiveMemoryCwd('/h1/east\\', h1Cwds)).assertEqual('/h1/east\\');
      expect(resolveEffectiveMemoryCwd('/h1/west', h1Cwds)).assertEqual('/h1/west');

      // 4. Hidden switch to a host with 0 registered directories stays empty with 0 RPC calls
      selectedCwd = resolveEffectiveMemoryCwd('', []);
      expect(selectedCwd).assertEqual('');
      expect(resolveEffectiveMemoryCwd('/h1/east', [])).assertEqual('');
      expect(bridge.listMemoriesCalls.length).assertEqual(2);
    });

    it('blocks stale Memory CWD chip and header control closures across h1->h2, h1->h2->h1, and onHidden->onShown while allowing fresh chips (R6-P1-2)', 0, () => {
      const bridge = new FakePr6CoreBridge();
      bridge.fleetJson = '{"revision":1,"hosts":[{"host_id":"h1","display_name":"Host 1","status":"connected"},{"host_id":"h2","display_name":"Host 2","status":"connected"}],"projects":[],"total_pending_approvals":0}';
      const store = createPr6TestStore(bridge);
      store.selectHost('h1');
      const ep1 = store.hostSelectionEpoch;

      const h1Cwds = ['/tmp/h1/east', '/tmp/h1/west', '/tmp/h1/space ', '/tmp/h1/bs\\'];
      const h2Cwds = ['/tmp/h2/repo'];

      // Capture h1's `west` directory chip & refresh/period controls at render time (visibleEpoch = 2, rootEpoch = 0)
      const h1WestChip: OrbitMemoryCwdOptionBinding = createMemoryCwdOptionBinding('h1', ep1, 0, 2, '/tmp/h1/west');
      const h1RefreshCtrl = createSettingsControlBinding('h1', ep1, 0, 2, 'memory_refresh');
      const h1UsagePeriodCtrl = createSettingsControlBinding('h1', ep1, 0, 2, 'usage_period');
      const h1McpScopeCtrl = createSettingsControlBinding('h1', ep1, 0, 2, 'mcp_scope');

      // Active on mounted visible h1
      expect(isMemoryCwdOptionBindingActive(h1WestChip, true, true, 2, 'h1', ep1, 0, h1Cwds)).assertTrue();
      expect(isSettingsControlBindingActive(h1RefreshCtrl, true, true, 2, 'h1', ep1, 0, 'memory_refresh')).assertTrue();
      expect(isSettingsControlBindingActive(h1UsagePeriodCtrl, true, true, 2, 'h1', ep1, 0, 'usage_period')).assertTrue();
      expect(isSettingsControlBindingActive(h1McpScopeCtrl, true, true, 2, 'h1', ep1, 0, 'mcp_scope')).assertTrue();

      // 1. Switch h1 -> h2 (h2 loaded its own repo, does NOT contain h1's west): ALL h1 closures rejected (0 RPC, 0 haptic, list intact)
      store.selectHost('h2');
      const ep2 = store.hostSelectionEpoch;
      expect(ep2 > ep1).assertTrue();
      expect(isMemoryCwdOptionBindingActive(h1WestChip, true, true, 2, 'h2', ep2, 1, h2Cwds)).assertFalse();
      expect(isSettingsControlBindingActive(h1RefreshCtrl, true, true, 2, 'h2', ep2, 1, 'memory_refresh')).assertFalse();
      expect(isSettingsControlBindingActive(h1UsagePeriodCtrl, true, true, 2, 'h2', ep2, 1, 'usage_period')).assertFalse();
      expect(isSettingsControlBindingActive(h1McpScopeCtrl, true, true, 2, 'h2', ep2, 1, 'mcp_scope')).assertFalse();

      // 2. Switch h1 -> h2 -> h1 (ABA): even though `/tmp/h1/west` is back in h1Cwds, old h1 chip & controls STILL rejected!
      store.selectHost('h1');
      const ep3 = store.hostSelectionEpoch;
      expect(ep3 > ep2).assertTrue();
      expect(isMemoryCwdOptionBindingActive(h1WestChip, true, true, 2, 'h1', ep3, 2, h1Cwds)).assertFalse();
      expect(isSettingsControlBindingActive(h1RefreshCtrl, true, true, 2, 'h1', ep3, 2, 'memory_refresh')).assertFalse();
      expect(isSettingsControlBindingActive(h1UsagePeriodCtrl, true, true, 2, 'h1', ep3, 2, 'usage_period')).assertFalse();
      expect(isSettingsControlBindingActive(h1McpScopeCtrl, true, true, 2, 'h1', ep3, 2, 'mcp_scope')).assertFalse();

      // 3. Same host h1: onHidden (visibleEpoch 3) -> onShown (visibleEpoch 4) -> pre-hide chip STILL rejected!
      expect(isMemoryCwdOptionBindingActive(h1WestChip, true, false, 3, 'h1', ep1, 0, h1Cwds)).assertFalse();
      expect(isMemoryCwdOptionBindingActive(h1WestChip, true, true, 4, 'h1', ep1, 0, h1Cwds)).assertFalse();
      expect(isMemoryCwdOptionBindingActive(h1WestChip, false, false, 5, 'h1', ep1, 0, h1Cwds)).assertFalse();

      // 4. Fresh rendered chips on live h1 (preserving trailing space & POSIX backslash verbatim) succeed!
      const freshSpaceChip = createMemoryCwdOptionBinding('h1', ep3, 2, 4, '/tmp/h1/space ');
      const freshBackslashChip = createMemoryCwdOptionBinding('h1', ep3, 2, 4, '/tmp/h1/bs\\');
      expect(isMemoryCwdOptionBindingActive(freshSpaceChip, true, true, 4, 'h1', ep3, 2, h1Cwds)).assertTrue();
      expect(isMemoryCwdOptionBindingActive(freshBackslashChip, true, true, 4, 'h1', ep3, 2, h1Cwds)).assertTrue();
      expect(freshSpaceChip.rawCwd).assertEqual('/tmp/h1/space ');
      expect(freshBackslashChip.rawCwd).assertEqual('/tmp/h1/bs\\');
      // Chip whose rawCwd is no longer in host's directory list is rejected even if epochs match
      const removedChip = createMemoryCwdOptionBinding('h1', ep3, 2, 4, '/tmp/h1/removed');
      expect(isMemoryCwdOptionBindingActive(removedChip, true, true, 4, 'h1', ep3, 2, h1Cwds)).assertFalse();
    });

    it('isolates activeCwd per host so h2 and 0-directory h3 never inherit h1 activeCwd in Memory/MCP/Skills (R7-P1-1)', 0, async () => {
      const bridge = new FakePr6CoreBridge();
      bridge.fleetJson = '{"revision":1,"hosts":[{"host_id":"h1","display_name":"Host 1","status":"connected"},{"host_id":"h2","display_name":"Host 2","status":"connected"},{"host_id":"h3","display_name":"Host 3","status":"connected"}],"projects":[{"host_id":"h1","cwd":"/tmp/h1/east "},{"host_id":"h2","cwd":"/tmp/h2/repo\\\\"}],"total_pending_approvals":0}';
      const store = createPr6TestStore(bridge);

      // 1. Select h1 and set activeCwd with trailing space verbatim
      store.selectHost('h1');
      store.setActiveCwd('/tmp/h1/east ');
      expect(store.activeCwd).assertEqual('/tmp/h1/east ');
      expect(store.getActiveCwdForHost('h1')).assertEqual('/tmp/h1/east ');
      expect(store.getActiveCwdForHost('h2')).assertEqual('');
      expect(store.getActiveCwdForHost('h3')).assertEqual('');

      // 2. Switch to h2 (has /tmp/h2/repo\): activeCwd resets off h1's path; Memory/MCP/Skills use h2's directory, NEVER /tmp/h1/east
      store.selectHost('h2');
      expect(store.activeCwd).assertEqual('~');
      expect(store.getActiveCwdForHost('h2')).assertEqual('');
      expect(store.getActiveCwdForHost('h1')).assertEqual('/tmp/h1/east ');
      const h2Cwds = ['/tmp/h2/repo\\'];
      const h2MemoryCwd = resolveEffectiveMemoryCwd('', h2Cwds);
      expect(h2MemoryCwd).assertEqual('/tmp/h2/repo\\');
      const h2McpCwd = resolveHostScopedExtensionCwdPure(
        'h2',
        store.selectedHostId,
        store.getActiveCwdForHost('h2'),
        h2Cwds,
        true,
        ''
      );
      expect(h2McpCwd).assertEqual('/tmp/h2/repo\\');
      await store.listExternalMcpServersScoped('h2', h2McpCwd);
      await store.listManagedSkillsScoped('h2', h2McpCwd);
      expect(bridge.listMcpCalls[0]).assertEqual('h2|/tmp/h2/repo\\');
      expect(bridge.listSkillsCalls[0]).assertEqual('h2|/tmp/h2/repo\\');

      // 3. Switch to 0-directory h3: activeCwd does NOT leak h1/h2; Memory stays empty (0 RPC), MCP/Skills degrade to host-only null!
      store.selectHost('h3');
      expect(store.activeCwd).assertEqual('~');
      expect(store.getActiveCwdForHost('h3')).assertEqual('');
      const h3Cwds: string[] = [];
      const h3MemoryCwd = resolveEffectiveMemoryCwd('', h3Cwds);
      expect(h3MemoryCwd).assertEqual('');
      const h3McpCwd = resolveHostScopedExtensionCwdPure(
        'h3',
        store.selectedHostId,
        store.getActiveCwdForHost('h3'),
        h3Cwds,
        true,
        ''
      );
      expect(h3McpCwd).assertNull();
      await store.listExternalMcpServersScoped('h3', h3McpCwd);
      await store.listManagedSkillsScoped('h3', h3McpCwd);
      expect(bridge.listMemoriesCalls.length).assertEqual(0);
      expect(bridge.listMcpCalls[1]).assertEqual('h3|');
      expect(bridge.listSkillsCalls[1]).assertEqual('h3|');

      // 4. Switch back to h1: h1's own exact untrimmed activeCwd is restored!
      store.selectHost('h1');
      expect(store.activeCwd).assertEqual('/tmp/h1/east ');
      expect(store.getActiveCwdForHost('h1')).assertEqual('/tmp/h1/east ');
    });

    it('blocks stale Host Detail route mode closures across onHidden, onHidden->onShown, aboutToDisappear, and same-instance h1->h2->h1 rebind (R7-P1-2)', 0, () => {
      const bridge = new FakePr6CoreBridge();
      bridge.fleetJson = '{"revision":1,"hosts":[{"host_id":"h1","display_name":"Host 1","status":"connected"},{"host_id":"h2","display_name":"Host 2","status":"connected"}],"projects":[],"total_pending_approvals":0}';
      const store = createPr6TestStore(bridge);
      store.selectHost('h1');
      const ep1 = store.hostSelectionEpoch;

      // Capture h1 route mode binding with hostBindEpoch = 1, visibleEpoch = 2
      const h1RouteCtrl = createSettingsControlBinding('h1', ep1, 1, 2, 'host_route_mode');
      expect(isSettingsControlBindingActive(h1RouteCtrl, true, true, 2, 'h1', ep1, 1, 'host_route_mode')).assertTrue();

      // 1. onHidden (visibleEpoch = 3) -> rejected
      expect(isSettingsControlBindingActive(h1RouteCtrl, true, false, 3, 'h1', ep1, 1, 'host_route_mode')).assertFalse();
      // 2. onHidden -> onShown (visibleEpoch = 4) -> pre-hide closure STILL rejected
      expect(isSettingsControlBindingActive(h1RouteCtrl, true, true, 4, 'h1', ep1, 1, 'host_route_mode')).assertFalse();
      // 3. aboutToDisappear -> rejected
      expect(isSettingsControlBindingActive(h1RouteCtrl, false, false, 5, 'h1', ep1, 1, 'host_route_mode')).assertFalse();
      // 4. Same-instance @Param hostId rebind h1 -> h2 WITHOUT selectHost (ep1 unchanged, hostBindEpoch = 2) -> rejected
      expect(isSettingsControlBindingActive(h1RouteCtrl, true, true, 2, 'h2', ep1, 2, 'host_route_mode')).assertFalse();
      // 5. Same-instance @Param hostId rebind h1 -> h2 -> h1 WITHOUT selectHost (ep1 unchanged, hostBindEpoch = 3) -> old h1 closure STILL rejected!
      expect(isSettingsControlBindingActive(h1RouteCtrl, true, true, 2, 'h1', ep1, 3, 'host_route_mode')).assertFalse();
      // 6. Fresh post-rebind h1 closure (hostBindEpoch = 3, visibleEpoch = 4) -> allowed!
      const freshH1RouteCtrl = createSettingsControlBinding('h1', ep1, 3, 4, 'host_route_mode');
      expect(isSettingsControlBindingActive(freshH1RouteCtrl, true, true, 4, 'h1', ep1, 3, 'host_route_mode')).assertTrue();
    });

    it('blocks stale Personal Agent refresh and config closures across aboutToDisappear, onHidden, h1->h2, and h1->h2->h1 (R7-P1-3)', 0, () => {
      const bridge = new FakePr6CoreBridge();
      bridge.fleetJson = '{"revision":1,"hosts":[{"host_id":"h1","display_name":"Host 1","status":"connected"},{"host_id":"h2","display_name":"Host 2","status":"connected"}],"projects":[],"total_pending_approvals":0}';
      const store = createPr6TestStore(bridge);
      store.selectHost('h1');
      const ep1 = store.hostSelectionEpoch;

      const h1PaRefresh = createSettingsControlBinding('h1', ep1, 0, 2, 'pa_refresh');
      const h1PaConfig = createSettingsControlBinding('h1', ep1, 0, 2, 'pa_config');
      expect(isSettingsControlBindingActive(h1PaRefresh, true, true, 2, 'h1', ep1, 0, 'pa_refresh')).assertTrue();
      expect(isSettingsControlBindingActive(h1PaConfig, true, true, 2, 'h1', ep1, 0, 'pa_config')).assertTrue();

      // 1. aboutToDisappear -> rejected
      expect(isSettingsControlBindingActive(h1PaRefresh, false, false, 3, 'h1', ep1, 0, 'pa_refresh')).assertFalse();
      expect(isSettingsControlBindingActive(h1PaConfig, false, false, 3, 'h1', ep1, 0, 'pa_config')).assertFalse();

      // 2. onHidden -> onShown -> pre-hide closures rejected
      expect(isSettingsControlBindingActive(h1PaRefresh, true, false, 3, 'h1', ep1, 0, 'pa_refresh')).assertFalse();
      expect(isSettingsControlBindingActive(h1PaRefresh, true, true, 4, 'h1', ep1, 0, 'pa_refresh')).assertFalse();

      // 3. Host switch h1 -> h2 -> rejected
      store.selectHost('h2');
      const ep2 = store.hostSelectionEpoch;
      expect(isSettingsControlBindingActive(h1PaRefresh, true, true, 2, 'h2', ep2, 1, 'pa_refresh')).assertFalse();
      expect(isSettingsControlBindingActive(h1PaConfig, true, true, 2, 'h2', ep2, 1, 'pa_config')).assertFalse();

      // 4. Host switch h1 -> h2 -> h1 (ABA) -> old h1 closures STILL rejected, fresh h1 closures accepted!
      store.selectHost('h1');
      const ep3 = store.hostSelectionEpoch;
      expect(isSettingsControlBindingActive(h1PaRefresh, true, true, 2, 'h1', ep3, 2, 'pa_refresh')).assertFalse();
      expect(isSettingsControlBindingActive(h1PaConfig, true, true, 2, 'h1', ep3, 2, 'pa_config')).assertFalse();
      const freshPaRefresh = createSettingsControlBinding('h1', ep3, 2, 4, 'pa_refresh');
      const freshPaConfig = createSettingsControlBinding('h1', ep3, 2, 4, 'pa_config');
      expect(isSettingsControlBindingActive(freshPaRefresh, true, true, 4, 'h1', ep3, 2, 'pa_refresh')).assertTrue();
      expect(isSettingsControlBindingActive(freshPaConfig, true, true, 4, 'h1', ep3, 2, 'pa_config')).assertTrue();
    });
  });
}
