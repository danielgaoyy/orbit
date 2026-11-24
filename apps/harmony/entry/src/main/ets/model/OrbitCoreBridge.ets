import { OrbitHarmonyCore } from 'liborbit_ohos.so';
import { util } from '@kit.ArkTS';
import { orbitLogInfo, orbitLogWarn } from './OrbitLogger';

export interface OrbitBridgeOpenOutcome {
  core: OrbitHarmonyCore;
  usedMasterKey: boolean;
}

/**
 * Thin ArkTS V2 bridge around the 225 N-API exports in `liborbit_ohos.so`.
 * Keeps FFI lifecycle, UTF-8 byte encoding, and HUKS-backed database opening isolated
 * from UI components while keeping Rust `orbit-client-core` as the single ViewModel layer.
 */
export class OrbitCoreBridge {
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
  }

  public raw(): OrbitHarmonyCore {
    return this.core;
  }

  public sendTerminalUtf8(hostId: string, slot: number, text: string): void {
    const encoder = new util.TextEncoder();
    const bytes = encoder.encodeInto(text);
    this.core.sendTerminalBytes(hostId, slot, bytes);
  }

  public sendTerminalBytes(hostId: string, slot: number, bytes: Uint8Array): void {
    this.core.sendTerminalBytes(hostId, slot, bytes);
  }

  public loadOlderAgentTimeline(hostId: string, agentId: string, limitEvents: number): boolean {
    return this.core.loadOlderAgentTimeline(hostId, agentId, limitEvents);
  }

  public async registerPushDeviceJson(hostId: string, reqJson: string): Promise<string> {
    return await this.core.registerPushDeviceJson(hostId, reqJson);
  }

  public async unregisterPushDeviceJson(hostId: string, deviceId: string): Promise<string> {
    return await this.core.unregisterPushDeviceJson(hostId, deviceId);
  }

  public registerPushTokenJson(platform: string, token: string): string {
    return this.core.registerPushTokenJson(platform, token);
  }

  public pushRegistrationStatusJson(): string {
    return this.core.pushRegistrationStatusJson();
  }

  public async updatePushPreferencesJson(hostId: string, reqJson: string): Promise<string> {
    return await this.core.updatePushPreferencesJson(hostId, reqJson);
  }

  public async testPushNotificationJson(hostId: string, deviceId: string | null): Promise<string> {
    return await this.core.testPushNotificationJson(hostId, deviceId);
  }

  public async resolvePermissionsBatchJson(
    hostId: string,
    itemsJson: string,
    decision: string
  ): Promise<string> {
    return await this.core.resolvePermissionsBatchJson(hostId, itemsJson, decision);
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
    await this.core.resolvePlanApprovalJson(
      hostId,
      agentId,
      permissionRequestId,
      actionJson,
      editedPlanMarkdown,
      feedback,
      targetModeJson
    );
  }

  public interruptAgentAndContinue(hostId: string, agentId: string): void {
    this.core.interruptAgentAndContinue(hostId, agentId);
  }

  public searchMessagesJson(query: string, limit: number): string {
    return this.core.searchMessagesJson(query, limit);
  }

  public async searchMessagesRemoteJson(reqJson: string): Promise<string> {
    return await this.core.searchMessagesRemoteJson(reqJson);
  }

  public async writeWorkspaceFileJson(reqJson: string): Promise<string> {
    return await this.core.writeWorkspaceFileJson(reqJson);
  }

  public async createWorkspaceEntryJson(reqJson: string): Promise<string> {
    return await this.core.createWorkspaceEntryJson(reqJson);
  }

  public async renameWorkspaceEntryJson(reqJson: string): Promise<string> {
    return await this.core.renameWorkspaceEntryJson(reqJson);
  }

  public async deleteWorkspaceEntryJson(reqJson: string): Promise<string> {
    return await this.core.deleteWorkspaceEntryJson(reqJson);
  }

  public async gitStagePathsJson(
    hostId: string,
    cwd: string,
    paths: Array<string>,
    all: boolean
  ): Promise<string> {
    return await this.core.gitStagePathsJson(hostId, cwd, paths, all);
  }

  public async gitUnstagePathsJson(
    hostId: string,
    cwd: string,
    paths: Array<string>,
    all: boolean
  ): Promise<string> {
    return await this.core.gitUnstagePathsJson(hostId, cwd, paths, all);
  }

  public async gitCommitJson(
    hostId: string,
    cwd: string,
    message: string,
    amend: boolean
  ): Promise<string> {
    return await this.core.gitCommitJson(hostId, cwd, message, amend);
  }

  public async gitGenerateCommitMessageJson(hostId: string, cwd: string): Promise<string> {
    return await this.core.gitGenerateCommitMessageJson(hostId, cwd);
  }

  public async gitPrStatusJson(hostId: string, cwd: string): Promise<string> {
    return await this.core.gitPrStatusJson(hostId, cwd);
  }

  public async gitCreatePrJson(
    hostId: string,
    cwd: string,
    title: string,
    body: string,
    base: string | null,
    draft: boolean
  ): Promise<string> {
    return await this.core.gitCreatePrJson(hostId, cwd, title, body, base, draft);
  }

  public async gitStashListJson(hostId: string, cwd: string): Promise<string> {
    return await this.core.gitStashListJson(hostId, cwd);
  }

  public async gitStashSaveJson(
    hostId: string,
    cwd: string,
    message: string | null,
    includeUntracked: boolean
  ): Promise<string> {
    return await this.core.gitStashSaveJson(hostId, cwd, message, includeUntracked);
  }

  public async gitStashPopJson(
    hostId: string,
    cwd: string,
    stashRef: string,
    expectedOid: string | null
  ): Promise<string> {
    return await this.core.gitStashPopJson(hostId, cwd, stashRef, expectedOid);
  }

  public async gitConflictsJson(hostId: string, cwd: string): Promise<string> {
    return await this.core.gitConflictsJson(hostId, cwd);
  }

  public async gitRebaseStatusJson(hostId: string, cwd: string): Promise<string> {
    return await this.core.gitRebaseStatusJson(hostId, cwd);
  }

  public async gitRebaseContinueJson(hostId: string, cwd: string): Promise<string> {
    return await this.core.gitRebaseContinueJson(hostId, cwd);
  }

  public async gitRebaseAbortJson(hostId: string, cwd: string): Promise<string> {
    return await this.core.gitRebaseAbortJson(hostId, cwd);
  }

  public async discoverFleetRouterHostsJson(): Promise<string> {
    return await this.core.discoverFleetRouterHostsJson();
  }

  public async getRouterConfigJson(hostId: string): Promise<string> {
    return await this.core.getRouterConfigJson(hostId);
  }

  public async updateRouterConfigJson(
    hostId: string,
    configJson: string,
    externalWebhookSecret: string | null,
    activate: boolean
  ): Promise<string> {
    return await this.core.updateRouterConfigJson(hostId, configJson, externalWebhookSecret, activate);
  }

  public async getFleetMeshSnapshotJson(): Promise<string> {
    return await this.core.getFleetMeshSnapshotJson();
  }

  public async updateMeshPeerJson(
    hostId: string,
    registrationId: string,
    label: string | null,
    enabled: boolean | null
  ): Promise<void> {
    await this.core.updateMeshPeerJson(hostId, registrationId, label, enabled);
  }

  public async getSessionUsageJson(hostId: string, agentId: string): Promise<string> {
    return await this.core.getSessionUsageJson(hostId, agentId);
  }

  public listProviderSubagents(hostId: string, parentAgentId: string): void {
    this.core.listProviderSubagents(hostId, parentAgentId);
  }

  public openProviderSubagentTimeline(hostId: string, parentAgentId: string, subagentId: string): void {
    this.core.openProviderSubagentTimeline(hostId, parentAgentId, subagentId);
  }

  public providerSubagentTimelineJson(hostId: string, parentAgentId: string, subagentId: string): string {
    return this.core.providerSubagentTimelineJson(hostId, parentAgentId, subagentId);
  }

  public cloudSyncExportDeviceLinkUri(): string {
    return this.core.cloudSyncExportDeviceLinkUri();
  }

  public async cloudSyncImportDeviceLinkUriJson(uri: string): Promise<string> {
    return await this.core.cloudSyncImportDeviceLinkUriJson(uri);
  }

  public async cloudSyncSetManualPassphraseModeJson(requireManual: boolean): Promise<string> {
    return await this.core.cloudSyncSetManualPassphraseModeJson(requireManual);
  }

  public async cloudSyncStartGithubDeviceFlowJson(clientIdOverride: string | null): Promise<string> {
    return await this.core.cloudSyncStartGithubDeviceFlowJson(clientIdOverride);
  }

  public async cloudSyncPollGithubDeviceFlowJson(
    deviceCode: string,
    clientIdOverride: string | null
  ): Promise<string> {
    return await this.core.cloudSyncPollGithubDeviceFlowJson(deviceCode, clientIdOverride);
  }

  public async startGoalJson(reqJson: string): Promise<string> {
    return await this.core.startGoalJson(reqJson);
  }

  public async createTimerJson(
    hostId: string,
    agentId: string,
    durationSeconds: number,
    prompt: string,
    conditionJson: string
  ): Promise<string> {
    return await this.core.createTimerJson(hostId, agentId, durationSeconds, prompt, conditionJson);
  }

  public async createMonitorJson(reqJson: string): Promise<string> {
    return await this.core.createMonitorJson(reqJson);
  }

  public async startWorkflowJson(reqJson: string): Promise<string> {
    return await this.core.startWorkflowJson(reqJson);
  }

  public async upgradeHostDaemonOverWire(hostId: string, targetVersion: string | null): Promise<string> {
    return await this.core.upgradeHostDaemonOverWire(hostId, targetVersion);
  }
}
