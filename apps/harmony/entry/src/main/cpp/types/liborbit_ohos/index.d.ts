export class OrbitHarmonyCore {
  constructor(dbPath: string, autoDiscoverLocal?: boolean | null);
  static openWithOptions(dbPath: string, autoDiscoverLocal: boolean): OrbitHarmonyCore;
  subscribeEvents(callback: (err: Error | null, payloadJson: string) => void): void;
  discoverLocalDaemonUri(): string | null;
  fleetOverviewJson(): string;
  bridgeInfoJson(): string;
  fleetSidebarByRequestJson(reqJson: string): string;
  agentStageDeltaByRequestJson(reqJson: string): string;
  createSessionJson(reqJson: string): string;
  sendPromptJson(reqJson: string): void;
  updateHostConfigRequestJson(reqJson: string): string;
  resolveModelSelectionJson(reqJson: string): string;
  slashCommandsJson(platform?: string | null, query?: string | null): string;
  composePromptJson(reqJson: string): string;
  rankMentionCandidatesJson(reqJson: string): string;
  hostDetailJson(hostId: string): string;
  agentStageJson(hostId: string, agentId: string): string;
  agentStageDeltaJson(hostId: string, agentId: string, sinceRevision: number): string;
  fleetSidebarJson(
    sort?: string | null,
    filter?: string | null,
    query?: string | null,
    activeHostId?: string | null,
    activeAgentId?: string | null,
    activeCwd?: string | null
  ): string;
  markAgentSeen(hostId: string, agentId: string, seq: number): void;
  markAgentUnread(hostId: string, agentId: string): void;
  setAgentPinned(hostId: string, agentId: string, pinned: boolean): void;
  togglePinAgent(hostId: string, agentId: string): boolean;
  seedThreadPrefsJson(prefsJson: string): void;
  fallbackProviderCatalogJson(provider: string): string;
  fallbackProviderCatalogsJson(): string;
  formatCatalogModelLabel(catalogJson: string, rawModelId?: string | null): string;
  formatEffortLabel(effort: string): string;
  defaultEffortDescription(effort: string): string;
  resolveSupportedEffortForModel(
    specJson?: string | null,
    desired?: string | null,
    catalogDefault?: string | null
  ): string | null;
  agentTimelineJson(hostId: string, agentId: string): string;
  agentTimelineDeltaJson(hostId: string, agentId: string, sinceSeq: number): string;
  describeToolItemJson(itemJson: string, cwd?: string | null): string | null;
  gitReviewJson(hostId: string, cwd: string, path?: string | null): string;
  terminalScreenJson(hostId: string): string;
  terminalScreenSlotJson(hostId: string, slot: number): string;
  pairHostJson(pairingUri: string): string;
  removeHost(hostId: string): void;
  updateHostRelay(hostId: string, relayUrl?: string | null): void;
  updateHostConfigJson(
    hostId: string,
    customName?: string | null,
    routeMode?: string | null,
    sshAlias?: string | null,
    remoteDaemonPort?: number | null,
    customDirectEndpoint?: string | null,
    relayUrl?: string | null,
    reconnect?: boolean | null
  ): string;
  upsertProjectJson(
    projectId: string | null | undefined,
    hostId: string,
    name: string,
    directoriesJson: string
  ): string;
  removeProject(projectId: string): void;
  listDirectory(hostId: string, path?: string | null): void;
  connectAllHosts(): void;
  connectHost(hostId: string): void;
  refreshHost(hostId: string): void;
  refreshQuotas(hostId: string): void;
  refreshProviderModels(hostId: string, forceRefresh: boolean): void;
  createAgent(
    hostId: string,
    cwd: string,
    provider: string,
    mode: string,
    initialPrompt: string,
    worktreeBranch?: string | null
  ): void;
  createAgentFull(
    hostId: string,
    cwd: string,
    extraDirsJson: string | null | undefined,
    provider: string,
    model: string | null | undefined,
    effort: string | null | undefined,
    mode: string,
    initialPrompt: string,
    initialDisplayPrompt?: string | null,
    initialBadgesJson?: string | null,
    worktreeBranch?: string | null
  ): string;
  openAgentTimeline(hostId: string, agentId: string): void;
  sendPrompt(hostId: string, agentId: string, prompt: string): void;
  sendPromptWithMeta(
    hostId: string,
    agentId: string,
    prompt: string,
    displayPrompt?: string | null,
    badgesJson?: string | null
  ): void;
  enqueuePrompt(
    hostId: string,
    agentId: string,
    prompt: string,
    displayPrompt?: string | null,
    badgesJson?: string | null,
    queueId?: string | null
  ): string;
  removeQueuedPrompt(hostId: string, agentId: string, queueId: string): void;
  flushQueuedPromptNow(hostId: string, agentId: string, queueId: string): void;
  reorderQueuedPrompts(hostId: string, agentId: string, queueIdsJson: string): void;
  interruptAgent(hostId: string, agentId: string): void;
  setSessionMode(hostId: string, agentId: string, mode: string): void;
  setAgentConfig(
    hostId: string,
    agentId: string,
    model?: string | null,
    effort?: string | null
  ): void;
  switchAgentProvider(
    hostId: string,
    agentId: string,
    provider: string,
    model?: string | null,
    effort?: string | null
  ): void;
  archiveAgent(hostId: string, agentId: string): void;
  resolvePermission(
    hostId: string,
    agentId: string,
    permissionRequestId: string,
    decision: string
  ): void;
  answerQuestion(
    hostId: string,
    agentId: string,
    questionRequestId: string,
    answersJson: string
  ): void;
  fetchGitStatus(hostId: string, cwd: string): void;
  fetchGitDiff(hostId: string, cwd: string, path: string | null | undefined, staged: boolean): void;
  fetchWorkspaceFiles(hostId: string, cwd: string): void;
  workspaceFilesJson(hostId: string, cwd: string): string;
  stageAttachmentJson(
    hostId: string,
    filename: string,
    mimeType: string | null | undefined,
    bytes: Uint8Array
  ): Promise<string>;
  startTerminal(hostId: string, cwd: string, cols: number, rows: number): void;
  attachTerminal(hostId: string, terminalId: string, slot: number): void;
  killTerminal(hostId: string, terminalId: string): void;
  sendTerminalKeys(hostId: string, keys: string): void;
  sendTerminalKeysSlot(hostId: string, slot: number, keys: string): void;
  resizeTerminal(hostId: string, cols: number, rows: number): void;
  resizeTerminalSlot(hostId: string, slot: number, cols: number, rows: number): void;
  rewindAgentJson(
    hostId: string,
    agentId: string,
    toBeforeSeq: number,
    restoreFiles: boolean
  ): Promise<string>;
  readWorkspaceFileJson(hostId: string, cwd: string, path: string): Promise<string>;
  buildSideQuestionContext(hostId: string, agentId: string, maxChars?: number | null): string;
  askSideQuestion(
    reqJson: string,
    callback: (err: Error | null, payloadJson: string) => void
  ): string;
  detectDiagramKind(language: string, source: string): string | null;
  renderDiagramJson(reqJson: string): Promise<string>;
  onAppForeground(): void;
  onAppBackground(): void;
  onNetworkChanged(): void;
}
