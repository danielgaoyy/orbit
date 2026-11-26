import { OrbitHarmonyCore } from 'liborbit_ohos.so';
import { util } from '@kit.ArkTS';
import { orbitLogInfo, orbitLogWarn } from './OrbitLogger';
import { OrbitPerfProbe } from './OrbitPerfProbe';

export interface OrbitBridgeOpenOutcome {
  core: OrbitHarmonyCore;
  usedMasterKey: boolean;
}

export interface OrbitStageBridge {
  agentStageDeltaByRequestJsonAsync(reqJson: string): Promise<string>;
  agentStageTurnsSliceJson(
    hostId: string,
    agentId: string,
    startTurnIndex: number,
    count: number
  ): string;
  loadOlderAgentTimeline(hostId: string, agentId: string, limitEvents: number): boolean;
}

export interface OrbitFleetBridge {
  fleetOverviewJson(): string;
  fleetOverviewJsonAsync(): Promise<string>;
  hostDetailJson(hostId: string): string;
  hostDetailJsonAsync(hostId: string): Promise<string>;
  fleetSidebarByRequestJson(reqJson: string): string;
  fleetSidebarByRequestJsonAsync(reqJson: string): Promise<string>;
}

export interface OrbitPersonalAgentBridge {
  personalAgentBriefingJson(primaryRouterHostId?: string | null): string;
  listPersonalAgentActivityJson(
    hostId: string,
    limit: number,
    sinceMs?: number | null
  ): Promise<string>;
}

export interface OrbitComposerDraftBridge {
  getComposerDraftJson(draftKey: string): string | null;
  setComposerDraft(draftKey: string, draftJson: string): void;
  deleteComposerDraft(draftKey: string): void;
}

/**
 * Thin ArkTS V2 bridge around the N-API exports in `liborbit_ohos.so`.
 * Keeps FFI lifecycle, UTF-8 byte encoding, HUKS-backed database opening, and
 * `OrbitPerfProbe` instrumentation centralized in a single boundary layer while
 * keeping Rust `orbit-client-core` as the single ViewModel layer.
 */
export class OrbitCoreBridge
  implements OrbitStageBridge, OrbitFleetBridge, OrbitPersonalAgentBridge, OrbitComposerDraftBridge {
  private readonly core: OrbitHarmonyCore;
  public readonly usedMasterKey: boolean;

  constructor(core: OrbitHarmonyCore, usedMasterKey: boolean) {
    this.core = core;
    this.usedMasterKey = usedMasterKey;
  }

  public static openWithMasterKeyOrFallback(
    dbPath: string,
    masterKeyB64: string
  ): OrbitCoreBridge {
    const metric = 'ffi.openWithMasterKeyOrFallback';
    OrbitPerfProbe.begin(metric);
    try {
      const trimmedKey = masterKeyB64.trim();
      if (trimmedKey.length > 0) {
        try {
          const secured = OrbitHarmonyCore.newWithMasterKey(dbPath, trimmedKey);
          orbitLogInfo('OrbitBridge', 'Opened OrbitHarmonyCore via newWithMasterKey');
          return new OrbitCoreBridge(secured, true);
        } catch (err) {
          orbitLogWarn('OrbitBridge', 'newWithMasterKey failed; falling back to openWithOptions', err as Object);
        }
      }
      const fallback = OrbitHarmonyCore.openWithOptions(dbPath, true);
      return new OrbitCoreBridge(fallback, false);
    } finally {
      OrbitPerfProbe.end(metric, 0);
    }
  }

  private traceJsonSync(name: string, fn: () => string): string {
    const metric = `ffi.${name}`;
    OrbitPerfProbe.begin(metric);
    let payloadBytes = 0;
    try {
      const result = fn();
      payloadBytes = result.length;
      return result;
    } finally {
      OrbitPerfProbe.end(metric, payloadBytes);
    }
  }

  private traceNullableStringSync(name: string, fn: () => string | null): string | null {
    const metric = `ffi.${name}`;
    OrbitPerfProbe.begin(metric);
    let payloadBytes = 0;
    try {
      const result = fn();
      payloadBytes = result !== null ? result.length : 0;
      return result;
    } finally {
      OrbitPerfProbe.end(metric, payloadBytes);
    }
  }

  private traceBoolSync(name: string, fn: () => boolean): boolean {
    const metric = `ffi.${name}`;
    OrbitPerfProbe.begin(metric);
    try {
      return fn();
    } finally {
      OrbitPerfProbe.end(metric, 0);
    }
  }

  private async traceJsonAsync(name: string, fn: () => Promise<string>): Promise<string> {
    const metric = `ffi.${name}`;
    OrbitPerfProbe.begin(metric);
    let payloadBytes = 0;
    try {
      const result = await fn();
      payloadBytes = result.length;
      return result;
    } finally {
      OrbitPerfProbe.end(metric, payloadBytes);
    }
  }

  private traceVoidSync(name: string, payloadBytes: number, fn: () => void): void {
    const metric = `ffi.${name}`;
    OrbitPerfProbe.begin(metric);
    try {
      fn();
    } finally {
      OrbitPerfProbe.end(metric, payloadBytes);
    }
  }

  private async traceVoidAsync(name: string, fn: () => Promise<void>): Promise<void> {
    const metric = `ffi.${name}`;
    OrbitPerfProbe.begin(metric);
    try {
      await fn();
    } finally {
      OrbitPerfProbe.end(metric, 0);
    }
  }

  public raw(): OrbitHarmonyCore {
    return this.core;
  }

  // Event Loop & Lifecycle

  public subscribeEvents(callback: (err: Error | null, payloadJson: string) => void): void {
    this.traceVoidSync('subscribeEvents', 0, (): void => {
      this.core.subscribeEvents((err: Error | null, payloadJson: string): void => {
        const metric = 'event.dispatch';
        OrbitPerfProbe.begin(metric);
        try {
          callback(err, payloadJson);
        } finally {
          OrbitPerfProbe.end(metric, payloadJson.length);
        }
      });
    });
  }

  public onAppForeground(): void {
    this.traceVoidSync('onAppForeground', 0, (): void => {
      this.core.onAppForeground();
    });
  }

  public onAppBackground(): void {
    this.traceVoidSync('onAppBackground', 0, (): void => {
      this.core.onAppBackground();
    });
  }

  public onNetworkChanged(): void {
    this.traceVoidSync('onNetworkChanged', 0, (): void => {
      this.core.onNetworkChanged();
    });
  }

  public debugReplayStageDeltasJson(hostId: string, agentId: string, framesJson: string): void {
    this.traceVoidSync('debugReplayStageDeltasJson', framesJson.length, (): void => {
      this.core.debugReplayStageDeltasJson(hostId, agentId, framesJson);
    });
  }

  // Fleet, Host, Sidebar & Stage Hot Paths

  public fleetOverviewJson(): string {
    return this.traceJsonSync('fleetOverviewJson', (): string => {
      return this.core.fleetOverviewJson();
    });
  }

  public async fleetOverviewJsonAsync(): Promise<string> {
    return await this.traceJsonAsync('fleetOverviewJsonAsync', async (): Promise<string> => {
      return await this.core.fleetOverviewJsonAsync();
    });
  }

  public hostDetailJson(hostId: string): string {
    return this.traceJsonSync('hostDetailJson', (): string => {
      return this.core.hostDetailJson(hostId);
    });
  }

  public async hostDetailJsonAsync(hostId: string): Promise<string> {
    return await this.traceJsonAsync('hostDetailJsonAsync', async (): Promise<string> => {
      return await this.core.hostDetailJsonAsync(hostId);
    });
  }

  public fleetSidebarByRequestJson(reqJson: string): string {
    return this.traceJsonSync('fleetSidebarByRequestJson', (): string => {
      return this.core.fleetSidebarByRequestJson(reqJson);
    });
  }

  public async fleetSidebarByRequestJsonAsync(reqJson: string): Promise<string> {
    return await this.traceJsonAsync('fleetSidebarByRequestJsonAsync', async (): Promise<string> => {
      return await this.core.fleetSidebarByRequestJsonAsync(reqJson);
    });
  }

  public fleetSidebarJson(
    sort?: string | null,
    filter?: string | null,
    query?: string | null,
    activeHostId?: string | null,
    activeAgentId?: string | null,
    activeCwd?: string | null
  ): string {
    return this.traceJsonSync('fleetSidebarJson', (): string => {
      return this.core.fleetSidebarJson(sort, filter, query, activeHostId, activeAgentId, activeCwd);
    });
  }

  public agentStageDeltaJson(hostId: string, agentId: string, sinceRevision: number): string {
    return this.traceJsonSync('agentStageDeltaJson', (): string => {
      return this.core.agentStageDeltaJson(hostId, agentId, sinceRevision);
    });
  }

  public agentStageDeltaByRequestJson(reqJson: string): string {
    return this.traceJsonSync('agentStageDeltaByRequestJson', (): string => {
      return this.core.agentStageDeltaByRequestJson(reqJson);
    });
  }

  public async agentStageDeltaByRequestJsonAsync(reqJson: string): Promise<string> {
    return await this.traceJsonAsync('agentStageDeltaByRequestJsonAsync', async (): Promise<string> => {
      return await this.core.agentStageDeltaByRequestJsonAsync(reqJson);
    });
  }

  public agentStageTurnsSliceJson(
    hostId: string,
    agentId: string,
    startTurnIndex: number,
    count: number
  ): string {
    return this.traceJsonSync('agentStageTurnsSliceJson', (): string => {
      return this.core.agentStageTurnsSliceJson(hostId, agentId, startTurnIndex, count);
    });
  }

  public personalAgentBriefingJson(primaryRouterHostId?: string | null): string {
    return this.traceJsonSync('personalAgentBriefingJson', (): string => {
      return this.core.personalAgentBriefingJson(primaryRouterHostId ?? null);
    });
  }

  public async listPersonalAgentActivityJson(
    hostId: string,
    limit: number,
    sinceMs?: number | null
  ): Promise<string> {
    return await this.traceJsonAsync('listPersonalAgentActivityJson', async (): Promise<string> => {
      return await this.core.listPersonalAgentActivityJson(hostId, limit, sinceMs ?? null);
    });
  }

  public getComposerDraftJson(draftKey: string): string | null {
    return this.traceNullableStringSync('getComposerDraftJson', (): string | null => {
      return this.core.getComposerDraftJson(draftKey);
    });
  }

  public setComposerDraft(draftKey: string, draftJson: string): void {
    this.traceVoidSync('setComposerDraft', draftJson.length, (): void => {
      this.core.setComposerDraft(draftKey, draftJson);
    });
  }

  public deleteComposerDraft(draftKey: string): void {
    this.traceVoidSync('deleteComposerDraft', 0, (): void => {
      this.core.deleteComposerDraft(draftKey);
    });
  }

  public connectAllHosts(): void {
    this.traceVoidSync('connectAllHosts', 0, (): void => {
      this.core.connectAllHosts();
    });
  }

  public connectHost(hostId: string): void {
    this.traceVoidSync('connectHost', 0, (): void => {
      this.core.connectHost(hostId);
    });
  }

  public refreshHost(hostId: string): void {
    this.traceVoidSync('refreshHost', 0, (): void => {
      this.core.refreshHost(hostId);
    });
  }

  public refreshQuotas(hostId: string): void {
    this.traceVoidSync('refreshQuotas', 0, (): void => {
      this.core.refreshQuotas(hostId);
    });
  }

  public refreshProviderModels(hostId: string, forceRefresh: boolean): void {
    this.traceVoidSync('refreshProviderModels', 0, (): void => {
      this.core.refreshProviderModels(hostId, forceRefresh);
    });
  }

  public discoverLocalDaemonUri(): string | null {
    return this.traceNullableStringSync('discoverLocalDaemonUri', (): string | null => {
      return this.core.discoverLocalDaemonUri();
    });
  }

  public pairHostJson(pairingUri: string): string {
    return this.traceJsonSync('pairHostJson', (): string => {
      return this.core.pairHostJson(pairingUri);
    });
  }

  public removeHost(hostId: string): void {
    this.traceVoidSync('removeHost', 0, (): void => {
      this.core.removeHost(hostId);
    });
  }

  public updateHostConfigJson(
    hostId: string,
    customName?: string | null,
    routeMode?: string | null,
    sshAlias?: string | null,
    remoteDaemonPort?: number | null,
    customDirectEndpoint?: string | null,
    relayUrl?: string | null,
    reconnect?: boolean | null
  ): string {
    return this.traceJsonSync('updateHostConfigJson', (): string => {
      return this.core.updateHostConfigJson(
        hostId,
        customName,
        routeMode,
        sshAlias,
        remoteDaemonPort,
        customDirectEndpoint,
        relayUrl,
        reconnect
      );
    });
  }

  public upsertProjectJson(
    projectId: string | null | undefined,
    hostId: string,
    name: string,
    directoriesJson: string
  ): string {
    return this.traceJsonSync('upsertProjectJson', (): string => {
      return this.core.upsertProjectJson(projectId, hostId, name, directoriesJson);
    });
  }

  public removeProject(projectId: string): void {
    this.traceVoidSync('removeProject', 0, (): void => {
      this.core.removeProject(projectId);
    });
  }

  public listDirectory(hostId: string, path?: string | null): void {
    this.traceVoidSync('listDirectory', 0, (): void => {
      this.core.listDirectory(hostId, path);
    });
  }

  // Threads, Preferences, Models & Composer

  public seedThreadPrefsJson(prefsJson: string): void {
    this.traceVoidSync('seedThreadPrefsJson', prefsJson.length, (): void => {
      this.core.seedThreadPrefsJson(prefsJson);
    });
  }

  public togglePinAgent(hostId: string, agentId: string): boolean {
    return this.traceBoolSync('togglePinAgent', (): boolean => {
      return this.core.togglePinAgent(hostId, agentId);
    });
  }

  public markAgentSeen(hostId: string, agentId: string, seq: number): void {
    this.traceVoidSync('markAgentSeen', 0, (): void => {
      this.core.markAgentSeen(hostId, agentId, seq);
    });
  }

  public markAgentRead(hostId: string, agentId: string): void {
    this.traceVoidSync('markAgentRead', 0, (): void => {
      this.core.markAgentRead(hostId, agentId);
    });
  }

  public markAgentUnread(hostId: string, agentId: string): void {
    this.traceVoidSync('markAgentUnread', 0, (): void => {
      this.core.markAgentUnread(hostId, agentId);
    });
  }

  public openAgentTimeline(hostId: string, agentId: string): void {
    this.traceVoidSync('openAgentTimeline', 0, (): void => {
      this.core.openAgentTimeline(hostId, agentId);
    });
  }

  public loadOlderAgentTimeline(hostId: string, agentId: string, limitEvents: number): boolean {
    return this.traceBoolSync('loadOlderAgentTimeline', (): boolean => {
      return this.core.loadOlderAgentTimeline(hostId, agentId, limitEvents);
    });
  }

  public fallbackProviderCatalogsJson(): string {
    return this.traceJsonSync('fallbackProviderCatalogsJson', (): string => {
      return this.core.fallbackProviderCatalogsJson();
    });
  }

  public fallbackProviderCatalogJson(provider: string): string {
    return this.traceJsonSync('fallbackProviderCatalogJson', (): string => {
      return this.core.fallbackProviderCatalogJson(provider);
    });
  }

  public resolveModelSelectionJson(reqJson: string): string {
    return this.traceJsonSync('resolveModelSelectionJson', (): string => {
      return this.core.resolveModelSelectionJson(reqJson);
    });
  }

  public defaultEffortDescription(effort: string): string {
    return this.traceJsonSync('defaultEffortDescription', (): string => {
      return this.core.defaultEffortDescription(effort);
    });
  }

  public slashCommandsJson(platform?: string | null, query?: string | null): string {
    return this.traceJsonSync('slashCommandsJson', (): string => {
      return this.core.slashCommandsJson(platform, query);
    });
  }

  public rankMentionCandidatesJson(reqJson: string): string {
    return this.traceJsonSync('rankMentionCandidatesJson', (): string => {
      return this.core.rankMentionCandidatesJson(reqJson);
    });
  }

  public composePromptJson(reqJson: string): string {
    return this.traceJsonSync('composePromptJson', (): string => {
      return this.core.composePromptJson(reqJson);
    });
  }

  public createSessionJson(reqJson: string): string {
    return this.traceJsonSync('createSessionJson', (): string => {
      return this.core.createSessionJson(reqJson);
    });
  }

  public sendPromptWithMeta(
    hostId: string,
    agentId: string,
    prompt: string,
    displayPrompt?: string | null,
    badgesJson?: string | null
  ): void {
    this.traceVoidSync('sendPromptWithMeta', prompt.length, (): void => {
      this.core.sendPromptWithMeta(hostId, agentId, prompt, displayPrompt, badgesJson);
    });
  }

  public enqueuePrompt(
    hostId: string,
    agentId: string,
    prompt: string,
    displayPrompt?: string | null,
    badgesJson?: string | null,
    queueId?: string | null
  ): string {
    return this.traceJsonSync('enqueuePrompt', (): string => {
      return this.core.enqueuePrompt(hostId, agentId, prompt, displayPrompt, badgesJson, queueId);
    });
  }

  public removeQueuedPrompt(hostId: string, agentId: string, queueId: string): void {
    this.traceVoidSync('removeQueuedPrompt', 0, (): void => {
      this.core.removeQueuedPrompt(hostId, agentId, queueId);
    });
  }

  public flushQueuedPromptNow(hostId: string, agentId: string, queueId: string): void {
    this.traceVoidSync('flushQueuedPromptNow', 0, (): void => {
      this.core.flushQueuedPromptNow(hostId, agentId, queueId);
    });
  }

  public interruptAgent(hostId: string, agentId: string): void {
    this.traceVoidSync('interruptAgent', 0, (): void => {
      this.core.interruptAgent(hostId, agentId);
    });
  }

  public interruptAgentAndContinue(hostId: string, agentId: string): void {
    this.traceVoidSync('interruptAgentAndContinue', 0, (): void => {
      this.core.interruptAgentAndContinue(hostId, agentId);
    });
  }

  public setSessionMode(hostId: string, agentId: string, mode: string): void {
    this.traceVoidSync('setSessionMode', 0, (): void => {
      this.core.setSessionMode(hostId, agentId, mode);
    });
  }

  public setAccountDefaultMode(mode: string, updatedAtMs: number = 0): void {
    this.traceVoidSync('setAccountDefaultMode', 0, (): void => {
      this.core.setAccountDefaultMode(mode, updatedAtMs);
    });
  }

  public setAgentConfig(
    hostId: string,
    agentId: string,
    model?: string | null,
    effort?: string | null
  ): void {
    this.traceVoidSync('setAgentConfig', 0, (): void => {
      this.core.setAgentConfig(hostId, agentId, model, effort);
    });
  }

  public setAgentFastMode(hostId: string, agentId: string, fastMode: boolean): void {
    this.traceVoidSync('setAgentFastMode', 0, (): void => {
      this.core.setAgentFastMode(hostId, agentId, fastMode);
    });
  }

  public switchAgentProvider(
    hostId: string,
    agentId: string,
    provider: string,
    model?: string | null,
    effort?: string | null,
    acpAgentId?: string | null
  ): void {
    this.traceVoidSync('switchAgentProvider', 0, (): void => {
      this.core.switchAgentProvider(hostId, agentId, provider, model, effort, acpAgentId);
    });
  }

  public archiveAgent(hostId: string, agentId: string): void {
    this.traceVoidSync('archiveAgent', 0, (): void => {
      this.core.archiveAgent(hostId, agentId);
    });
  }

  public unarchiveAgent(hostId: string, agentId: string): void {
    this.traceVoidSync('unarchiveAgent', 0, (): void => {
      this.core.unarchiveAgent(hostId, agentId);
    });
  }

  public deleteAgent(hostId: string, agentId: string): void {
    this.traceVoidSync('deleteAgent', 0, (): void => {
      this.core.deleteAgent(hostId, agentId);
    });
  }

  public resolvePermission(
    hostId: string,
    agentId: string,
    permissionRequestId: string,
    decision: string
  ): void {
    this.traceVoidSync('resolvePermission', 0, (): void => {
      this.core.resolvePermission(hostId, agentId, permissionRequestId, decision);
    });
  }

  public answerQuestion(
    hostId: string,
    agentId: string,
    questionRequestId: string,
    answersJson: string
  ): void {
    this.traceVoidSync('answerQuestion', answersJson.length, (): void => {
      this.core.answerQuestion(hostId, agentId, questionRequestId, answersJson);
    });
  }

  public async stageAttachmentJson(
    hostId: string,
    filename: string,
    mimeType: string | null | undefined,
    bytes: Uint8Array
  ): Promise<string> {
    return await this.traceJsonAsync('stageAttachmentJson', async (): Promise<string> => {
      return await this.core.stageAttachmentJson(hostId, filename, mimeType, bytes);
    });
  }

  public async rewindAgentJson(
    hostId: string,
    agentId: string,
    toBeforeSeq: number,
    restoreFiles: boolean
  ): Promise<string> {
    return await this.traceJsonAsync('rewindAgentJson', async (): Promise<string> => {
      return await this.core.rewindAgentJson(hostId, agentId, toBeforeSeq, restoreFiles);
    });
  }

  public async forkAgentJson(reqJson: string): Promise<string> {
    return await this.traceJsonAsync('forkAgentJson', async (): Promise<string> => {
      return await this.core.forkAgentJson(reqJson);
    });
  }

  public exportAgentTranscriptJson(hostId: string, agentId: string, optionsJson: string): string {
    return this.traceJsonSync('exportAgentTranscriptJson', (): string => {
      return this.core.exportAgentTranscriptJson(hostId, agentId, optionsJson);
    });
  }

  public buildSideQuestionContext(hostId: string, agentId: string, maxChars?: number | null): string {
    return this.traceJsonSync('buildSideQuestionContext', (): string => {
      return this.core.buildSideQuestionContext(hostId, agentId, maxChars);
    });
  }

  public askSideQuestion(
    reqJson: string,
    callback: (err: Error | null, payloadJson: string) => void
  ): string {
    return this.traceJsonSync('askSideQuestion', (): string => {
      return this.core.askSideQuestion(reqJson, (err: Error | null, payloadJson: string): void => {
        const metric = 'event.sideQuestion';
        OrbitPerfProbe.begin(metric);
        try {
          callback(err, payloadJson);
        } finally {
          OrbitPerfProbe.end(metric, payloadJson.length);
        }
      });
    });
  }

  public detectDiagramKind(language: string, source: string): string | null {
    return this.traceNullableStringSync('detectDiagramKind', (): string | null => {
      return this.core.detectDiagramKind(language, source);
    });
  }

  public async renderDiagramJson(reqJson: string): Promise<string> {
    return await this.traceJsonAsync('renderDiagramJson', async (): Promise<string> => {
      return await this.core.renderDiagramJson(reqJson);
    });
  }

  // Terminal

  public startTerminal(hostId: string, cwd: string, cols: number, rows: number): void {
    this.traceVoidSync('startTerminal', 0, (): void => {
      this.core.startTerminal(hostId, cwd, cols, rows);
    });
  }

  public attachTerminal(hostId: string, terminalId: string, slot: number): void {
    this.traceVoidSync('attachTerminal', 0, (): void => {
      this.core.attachTerminal(hostId, terminalId, slot);
    });
  }

  public killTerminal(hostId: string, terminalId: string): void {
    this.traceVoidSync('killTerminal', 0, (): void => {
      this.core.killTerminal(hostId, terminalId);
    });
  }

  public resizeTerminal(hostId: string, cols: number, rows: number): void {
    this.traceVoidSync('resizeTerminal', 0, (): void => {
      this.core.resizeTerminal(hostId, cols, rows);
    });
  }

  public terminalScreenJson(hostId: string): string {
    return this.traceJsonSync('terminalScreenJson', (): string => {
      return this.core.terminalScreenJson(hostId);
    });
  }

  public terminalScreenSlotJson(hostId: string, slot: number): string {
    return this.traceJsonSync('terminalScreenSlotJson', (): string => {
      return this.core.terminalScreenSlotJson(hostId, slot);
    });
  }

  public sendTerminalUtf8(hostId: string, slot: number, text: string): void {
    const encoder = new util.TextEncoder();
    const bytes = encoder.encodeInto(text);
    this.traceVoidSync('sendTerminalUtf8', bytes.byteLength, (): void => {
      this.core.sendTerminalBytes(hostId, slot, bytes);
    });
  }

  public sendTerminalBytes(hostId: string, slot: number, bytes: Uint8Array): void {
    this.traceVoidSync('sendTerminalBytes', bytes.byteLength, (): void => {
      this.core.sendTerminalBytes(hostId, slot, bytes);
    });
  }

  // Push & Approvals

  public async registerPushDeviceJson(hostId: string, reqJson: string): Promise<string> {
    return await this.traceJsonAsync('registerPushDeviceJson', async (): Promise<string> => {
      return await this.core.registerPushDeviceJson(hostId, reqJson);
    });
  }

  public async unregisterPushDeviceJson(hostId: string, deviceId: string): Promise<string> {
    return await this.traceJsonAsync('unregisterPushDeviceJson', async (): Promise<string> => {
      return await this.core.unregisterPushDeviceJson(hostId, deviceId);
    });
  }

  public registerPushTokenJson(platform: string, token: string): string {
    return this.traceJsonSync('registerPushTokenJson', (): string => {
      return this.core.registerPushTokenJson(platform, token);
    });
  }

  public pushRegistrationStatusJson(): string {
    return this.traceJsonSync('pushRegistrationStatusJson', (): string => {
      return this.core.pushRegistrationStatusJson();
    });
  }

  public async updatePushPreferencesJson(hostId: string, reqJson: string): Promise<string> {
    return await this.traceJsonAsync('updatePushPreferencesJson', async (): Promise<string> => {
      return await this.core.updatePushPreferencesJson(hostId, reqJson);
    });
  }

  public async testPushNotificationJson(hostId: string, deviceId: string | null): Promise<string> {
    return await this.traceJsonAsync('testPushNotificationJson', async (): Promise<string> => {
      return await this.core.testPushNotificationJson(hostId, deviceId);
    });
  }

  public async resolvePermissionsBatchJson(
    hostId: string,
    itemsJson: string,
    decision: string
  ): Promise<string> {
    return await this.traceJsonAsync('resolvePermissionsBatchJson', async (): Promise<string> => {
      return await this.core.resolvePermissionsBatchJson(hostId, itemsJson, decision);
    });
  }

  public async resolvePlanApprovalJson(
    hostId: string,
    agentId: string,
    permissionRequestId: string,
    actionJson: string,
    editedPlanMarkdown: string | null,
    feedback: string | null,
    targetModeJson: string
  ): Promise<void> {
    await this.traceVoidAsync('resolvePlanApprovalJson', async (): Promise<void> => {
      await this.core.resolvePlanApprovalJson(
        hostId,
        agentId,
        permissionRequestId,
        actionJson,
        editedPlanMarkdown,
        feedback,
        targetModeJson
      );
    });
  }

  // Search, Files, Workspace Board & Peers

  public searchMessagesJson(query: string, limit: number): string {
    return this.traceJsonSync('searchMessagesJson', (): string => {
      return this.core.searchMessagesJson(query, limit);
    });
  }

  public async searchMessagesRemoteJson(reqJson: string): Promise<string> {
    return await this.traceJsonAsync('searchMessagesRemoteJson', async (): Promise<string> => {
      return await this.core.searchMessagesRemoteJson(reqJson);
    });
  }

  public fetchWorkspaceFiles(hostId: string, cwd: string): void {
    this.traceVoidSync('fetchWorkspaceFiles', 0, (): void => {
      this.core.fetchWorkspaceFiles(hostId, cwd);
    });
  }

  public workspaceFilesJson(hostId: string, cwd: string): string {
    return this.traceJsonSync('workspaceFilesJson', (): string => {
      return this.core.workspaceFilesJson(hostId, cwd);
    });
  }

  public async readWorkspaceFileJson(hostId: string, cwd: string, path: string): Promise<string> {
    return await this.traceJsonAsync('readWorkspaceFileJson', async (): Promise<string> => {
      return await this.core.readWorkspaceFileJson(hostId, cwd, path);
    });
  }

  public async writeWorkspaceFileJson(reqJson: string): Promise<string> {
    return await this.traceJsonAsync('writeWorkspaceFileJson', async (): Promise<string> => {
      return await this.core.writeWorkspaceFileJson(reqJson);
    });
  }

  public async createWorkspaceEntryJson(reqJson: string): Promise<string> {
    return await this.traceJsonAsync('createWorkspaceEntryJson', async (): Promise<string> => {
      return await this.core.createWorkspaceEntryJson(reqJson);
    });
  }

  public async renameWorkspaceEntryJson(reqJson: string): Promise<string> {
    return await this.traceJsonAsync('renameWorkspaceEntryJson', async (): Promise<string> => {
      return await this.core.renameWorkspaceEntryJson(reqJson);
    });
  }

  public async deleteWorkspaceEntryJson(reqJson: string): Promise<string> {
    return await this.traceJsonAsync('deleteWorkspaceEntryJson', async (): Promise<string> => {
      return await this.core.deleteWorkspaceEntryJson(reqJson);
    });
  }

  public fetchWorkspaceBoard(hostId: string, workspaceId: string): void {
    this.traceVoidSync('fetchWorkspaceBoard', 0, (): void => {
      this.core.fetchWorkspaceBoard(hostId, workspaceId);
    });
  }

  public workspaceBoardJson(hostId: string, workspaceId: string): string {
    return this.traceJsonSync('workspaceBoardJson', (): string => {
      return this.core.workspaceBoardJson(hostId, workspaceId);
    });
  }

  public updateWorkspaceBoard(
    hostId: string,
    workspaceId: string,
    content: string,
    section?: string | null,
    updatedBy?: string | null
  ): void {
    this.traceVoidSync('updateWorkspaceBoard', content.length, (): void => {
      this.core.updateWorkspaceBoard(hostId, workspaceId, content, section, updatedBy);
    });
  }

  public fetchWorkspaceEvents(hostId: string, workspaceId: string, limit?: number | null): void {
    this.traceVoidSync('fetchWorkspaceEvents', 0, (): void => {
      this.core.fetchWorkspaceEvents(hostId, workspaceId, limit);
    });
  }

  public workspaceEventsJson(hostId: string, workspaceId: string): string {
    return this.traceJsonSync('workspaceEventsJson', (): string => {
      return this.core.workspaceEventsJson(hostId, workspaceId);
    });
  }

  public spawnWorkspacePeerJson(reqJson: string): string {
    return this.traceJsonSync('spawnWorkspacePeerJson', (): string => {
      return this.core.spawnWorkspacePeerJson(reqJson);
    });
  }

  // Git

  public fetchGitStatus(hostId: string, cwd: string): void {
    this.traceVoidSync('fetchGitStatus', 0, (): void => {
      this.core.fetchGitStatus(hostId, cwd);
    });
  }

  public fetchGitDiff(
    hostId: string,
    cwd: string,
    path: string | null | undefined,
    staged: boolean
  ): void {
    this.traceVoidSync('fetchGitDiff', 0, (): void => {
      this.core.fetchGitDiff(hostId, cwd, path, staged);
    });
  }

  public gitReviewJson(hostId: string, cwd: string, path?: string | null): string {
    return this.traceJsonSync('gitReviewJson', (): string => {
      return this.core.gitReviewJson(hostId, cwd, path);
    });
  }

  public async gitStagePathsJson(
    hostId: string,
    cwd: string,
    paths: Array<string>,
    all: boolean
  ): Promise<string> {
    return await this.traceJsonAsync('gitStagePathsJson', async (): Promise<string> => {
      return await this.core.gitStagePathsJson(hostId, cwd, paths, all);
    });
  }

  public async gitUnstagePathsJson(
    hostId: string,
    cwd: string,
    paths: Array<string>,
    all: boolean
  ): Promise<string> {
    return await this.traceJsonAsync('gitUnstagePathsJson', async (): Promise<string> => {
      return await this.core.gitUnstagePathsJson(hostId, cwd, paths, all);
    });
  }

  public async gitCommitJson(
    hostId: string,
    cwd: string,
    message: string,
    amend: boolean
  ): Promise<string> {
    return await this.traceJsonAsync('gitCommitJson', async (): Promise<string> => {
      return await this.core.gitCommitJson(hostId, cwd, message, amend);
    });
  }

  public async gitGenerateCommitMessageJson(hostId: string, cwd: string): Promise<string> {
    return await this.traceJsonAsync('gitGenerateCommitMessageJson', async (): Promise<string> => {
      return await this.core.gitGenerateCommitMessageJson(hostId, cwd);
    });
  }

  public async gitPrStatusJson(hostId: string, cwd: string): Promise<string> {
    return await this.traceJsonAsync('gitPrStatusJson', async (): Promise<string> => {
      return await this.core.gitPrStatusJson(hostId, cwd);
    });
  }

  public async gitCreatePrJson(
    hostId: string,
    cwd: string,
    title: string,
    body: string,
    base: string | null,
    draft: boolean
  ): Promise<string> {
    return await this.traceJsonAsync('gitCreatePrJson', async (): Promise<string> => {
      return await this.core.gitCreatePrJson(hostId, cwd, title, body, base, draft);
    });
  }

  public async gitStashListJson(hostId: string, cwd: string): Promise<string> {
    return await this.traceJsonAsync('gitStashListJson', async (): Promise<string> => {
      return await this.core.gitStashListJson(hostId, cwd);
    });
  }

  public async gitStashSaveJson(
    hostId: string,
    cwd: string,
    message: string | null,
    includeUntracked: boolean
  ): Promise<string> {
    return await this.traceJsonAsync('gitStashSaveJson', async (): Promise<string> => {
      return await this.core.gitStashSaveJson(hostId, cwd, message, includeUntracked);
    });
  }

  public async gitStashPopJson(
    hostId: string,
    cwd: string,
    stashRef: string,
    expectedOid: string | null
  ): Promise<string> {
    return await this.traceJsonAsync('gitStashPopJson', async (): Promise<string> => {
      return await this.core.gitStashPopJson(hostId, cwd, stashRef, expectedOid);
    });
  }

  public async gitConflictsJson(hostId: string, cwd: string): Promise<string> {
    return await this.traceJsonAsync('gitConflictsJson', async (): Promise<string> => {
      return await this.core.gitConflictsJson(hostId, cwd);
    });
  }

  public async gitRebaseStatusJson(hostId: string, cwd: string): Promise<string> {
    return await this.traceJsonAsync('gitRebaseStatusJson', async (): Promise<string> => {
      return await this.core.gitRebaseStatusJson(hostId, cwd);
    });
  }

  public async gitRebaseContinueJson(hostId: string, cwd: string): Promise<string> {
    return await this.traceJsonAsync('gitRebaseContinueJson', async (): Promise<string> => {
      return await this.core.gitRebaseContinueJson(hostId, cwd);
    });
  }

  public async gitRebaseAbortJson(hostId: string, cwd: string): Promise<string> {
    return await this.traceJsonAsync('gitRebaseAbortJson', async (): Promise<string> => {
      return await this.core.gitRebaseAbortJson(hostId, cwd);
    });
  }

  // Router, Mesh, Usage & Subagents

  public async discoverFleetRouterHostsJson(): Promise<string> {
    return await this.traceJsonAsync('discoverFleetRouterHostsJson', async (): Promise<string> => {
      return await this.core.discoverFleetRouterHostsJson();
    });
  }

  public async getRouterConfigJson(hostId: string): Promise<string> {
    return await this.traceJsonAsync('getRouterConfigJson', async (): Promise<string> => {
      return await this.core.getRouterConfigJson(hostId);
    });
  }

  public async updateRouterConfigJson(
    hostId: string,
    configJson: string,
    externalWebhookSecret: string | null,
    activate: boolean
  ): Promise<string> {
    return await this.traceJsonAsync('updateRouterConfigJson', async (): Promise<string> => {
      return await this.core.updateRouterConfigJson(hostId, configJson, externalWebhookSecret, activate);
    });
  }

  public async getFleetMeshSnapshotJson(): Promise<string> {
    return await this.traceJsonAsync('getFleetMeshSnapshotJson', async (): Promise<string> => {
      return await this.core.getFleetMeshSnapshotJson();
    });
  }

  public async updateMeshPeerJson(
    hostId: string,
    registrationId: string,
    label: string | null,
    enabled: boolean | null
  ): Promise<void> {
    await this.traceVoidAsync('updateMeshPeerJson', async (): Promise<void> => {
      await this.core.updateMeshPeerJson(hostId, registrationId, label, enabled);
    });
  }

  public async getSessionUsageJson(hostId: string, agentId: string): Promise<string> {
    return await this.traceJsonAsync('getSessionUsageJson', async (): Promise<string> => {
      return await this.core.getSessionUsageJson(hostId, agentId);
    });
  }

  public listProviderSubagents(hostId: string, parentAgentId: string): void {
    this.traceVoidSync('listProviderSubagents', 0, (): void => {
      this.core.listProviderSubagents(hostId, parentAgentId);
    });
  }

  public openProviderSubagentTimeline(hostId: string, parentAgentId: string, subagentId: string): void {
    this.traceVoidSync('openProviderSubagentTimeline', 0, (): void => {
      this.core.openProviderSubagentTimeline(hostId, parentAgentId, subagentId);
    });
  }

  public providerSubagentTimelineJson(hostId: string, parentAgentId: string, subagentId: string): string {
    return this.traceJsonSync('providerSubagentTimelineJson', (): string => {
      return this.core.providerSubagentTimelineJson(hostId, parentAgentId, subagentId);
    });
  }

  // Cloud Sync

  public cloudSyncStatusJson(): string {
    return this.traceJsonSync('cloudSyncStatusJson', (): string => {
      return this.core.cloudSyncStatusJson();
    });
  }

  public cloudSyncGeneratePassphrase(): string {
    return this.traceJsonSync('cloudSyncGeneratePassphrase', (): string => {
      return this.core.cloudSyncGeneratePassphrase();
    });
  }

  public cloudSyncSetAutoSyncJson(enabled: boolean): string {
    return this.traceJsonSync('cloudSyncSetAutoSyncJson', (): string => {
      return this.core.cloudSyncSetAutoSyncJson(enabled);
    });
  }

  public async cloudSyncLoginWithTokenJson(token: string): Promise<string> {
    return await this.traceJsonAsync('cloudSyncLoginWithTokenJson', async (): Promise<string> => {
      return await this.core.cloudSyncLoginWithTokenJson(token);
    });
  }

  public async cloudSyncUnlockWithPassphraseJson(passphrase: string): Promise<string> {
    return await this.traceJsonAsync('cloudSyncUnlockWithPassphraseJson', async (): Promise<string> => {
      return await this.core.cloudSyncUnlockWithPassphraseJson(passphrase);
    });
  }

  public async cloudSyncNowJson(): Promise<string> {
    return await this.traceJsonAsync('cloudSyncNowJson', async (): Promise<string> => {
      return await this.core.cloudSyncNowJson();
    });
  }

  public cloudSyncLogoutJson(clearLocalHosts: boolean): string {
    return this.traceJsonSync('cloudSyncLogoutJson', (): string => {
      return this.core.cloudSyncLogoutJson(clearLocalHosts);
    });
  }

  public cloudSyncExportDeviceLinkUri(): string {
    return this.traceJsonSync('cloudSyncExportDeviceLinkUri', (): string => {
      return this.core.cloudSyncExportDeviceLinkUri();
    });
  }

  public async cloudSyncImportDeviceLinkUriJson(uri: string): Promise<string> {
    return await this.traceJsonAsync('cloudSyncImportDeviceLinkUriJson', async (): Promise<string> => {
      return await this.core.cloudSyncImportDeviceLinkUriJson(uri);
    });
  }

  public async cloudSyncSetManualPassphraseModeJson(requireManual: boolean): Promise<string> {
    return await this.traceJsonAsync('cloudSyncSetManualPassphraseModeJson', async (): Promise<string> => {
      return await this.core.cloudSyncSetManualPassphraseModeJson(requireManual);
    });
  }

  public async cloudSyncStartGithubDeviceFlowJson(clientIdOverride: string | null): Promise<string> {
    return await this.traceJsonAsync('cloudSyncStartGithubDeviceFlowJson', async (): Promise<string> => {
      return await this.core.cloudSyncStartGithubDeviceFlowJson(clientIdOverride);
    });
  }

  public async cloudSyncPollGithubDeviceFlowJson(
    deviceCode: string,
    clientIdOverride: string | null
  ): Promise<string> {
    return await this.traceJsonAsync('cloudSyncPollGithubDeviceFlowJson', async (): Promise<string> => {
      return await this.core.cloudSyncPollGithubDeviceFlowJson(deviceCode, clientIdOverride);
    });
  }

  // Automations (Schedules, Timers, Goals, Monitors, Workflows) & Daemon Upgrade

  public createScheduleJson(reqJson: string): void {
    this.traceVoidSync('createScheduleJson', reqJson.length, (): void => {
      this.core.createScheduleJson(reqJson);
    });
  }

  public pauseSchedule(hostId: string, scheduleId: string): void {
    this.traceVoidSync('pauseSchedule', 0, (): void => {
      this.core.pauseSchedule(hostId, scheduleId);
    });
  }

  public resumeSchedule(hostId: string, scheduleId: string): void {
    this.traceVoidSync('resumeSchedule', 0, (): void => {
      this.core.resumeSchedule(hostId, scheduleId);
    });
  }

  public runScheduleOnce(hostId: string, scheduleId: string): void {
    this.traceVoidSync('runScheduleOnce', 0, (): void => {
      this.core.runScheduleOnce(hostId, scheduleId);
    });
  }

  public deleteSchedule(hostId: string, scheduleId: string): void {
    this.traceVoidSync('deleteSchedule', 0, (): void => {
      this.core.deleteSchedule(hostId, scheduleId);
    });
  }

  public fetchScheduleRuns(hostId: string, scheduleId: string, limit?: number | null): void {
    this.traceVoidSync('fetchScheduleRuns', 0, (): void => {
      this.core.fetchScheduleRuns(hostId, scheduleId, limit);
    });
  }

  public scheduleRunsJson(hostId: string, scheduleId: string): string {
    return this.traceJsonSync('scheduleRunsJson', (): string => {
      return this.core.scheduleRunsJson(hostId, scheduleId);
    });
  }

  public async listConditionalTimersJson(
    hostId: string,
    agentId: string | null | undefined,
    includeResolved: boolean
  ): Promise<string> {
    return await this.traceJsonAsync('listConditionalTimersJson', async (): Promise<string> => {
      return await this.core.listConditionalTimersJson(hostId, agentId, includeResolved);
    });
  }

  public async cancelConditionalTimerJson(hostId: string, timerId: string): Promise<string> {
    return await this.traceJsonAsync('cancelConditionalTimerJson', async (): Promise<string> => {
      return await this.core.cancelConditionalTimerJson(hostId, timerId);
    });
  }

  public async fireConditionalTimerNowJson(hostId: string, timerId: string): Promise<string> {
    return await this.traceJsonAsync('fireConditionalTimerNowJson', async (): Promise<string> => {
      return await this.core.fireConditionalTimerNowJson(hostId, timerId);
    });
  }

  public async listGoalRunsJson(hostId: string, agentId?: string | null): Promise<string> {
    return await this.traceJsonAsync('listGoalRunsJson', async (): Promise<string> => {
      return await this.core.listGoalRunsJson(hostId, agentId);
    });
  }

  public async controlGoalRunJson(reqJson: string): Promise<string> {
    return await this.traceJsonAsync('controlGoalRunJson', async (): Promise<string> => {
      return await this.core.controlGoalRunJson(reqJson);
    });
  }

  public async listMonitorWatchesJson(hostId: string, agentId?: string | null): Promise<string> {
    return await this.traceJsonAsync('listMonitorWatchesJson', async (): Promise<string> => {
      return await this.core.listMonitorWatchesJson(hostId, agentId);
    });
  }

  public async stopMonitorWatchJson(reqJson: string): Promise<string> {
    return await this.traceJsonAsync('stopMonitorWatchJson', async (): Promise<string> => {
      return await this.core.stopMonitorWatchJson(reqJson);
    });
  }

  public async listWorkflowRunsJson(hostId: string, includeCompleted: boolean): Promise<string> {
    return await this.traceJsonAsync('listWorkflowRunsJson', async (): Promise<string> => {
      return await this.core.listWorkflowRunsJson(hostId, includeCompleted);
    });
  }

  public async cancelWorkflowRunJson(reqJson: string): Promise<string> {
    return await this.traceJsonAsync('cancelWorkflowRunJson', async (): Promise<string> => {
      return await this.core.cancelWorkflowRunJson(reqJson);
    });
  }

  public async startGoalJson(reqJson: string): Promise<string> {
    return await this.traceJsonAsync('startGoalJson', async (): Promise<string> => {
      return await this.core.startGoalJson(reqJson);
    });
  }

  public async createTimerJson(
    hostId: string,
    agentId: string,
    durationSeconds: number,
    prompt: string,
    conditionJson: string
  ): Promise<string> {
    return await this.traceJsonAsync('createTimerJson', async (): Promise<string> => {
      return await this.core.createTimerJson(hostId, agentId, durationSeconds, prompt, conditionJson);
    });
  }

  public async createMonitorJson(reqJson: string): Promise<string> {
    return await this.traceJsonAsync('createMonitorJson', async (): Promise<string> => {
      return await this.core.createMonitorJson(reqJson);
    });
  }

  public async startWorkflowJson(reqJson: string): Promise<string> {
    return await this.traceJsonAsync('startWorkflowJson', async (): Promise<string> => {
      return await this.core.startWorkflowJson(reqJson);
    });
  }

  public async upgradeHostDaemonOverWire(hostId: string, targetVersion: string | null): Promise<string> {
    return await this.traceJsonAsync('upgradeHostDaemonOverWire', async (): Promise<string> => {
      return await this.core.upgradeHostDaemonOverWire(hostId, targetVersion);
    });
  }
}
