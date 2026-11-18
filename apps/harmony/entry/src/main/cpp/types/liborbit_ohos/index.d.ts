export class OrbitHarmonyCore {
  constructor(dbPath: string);
  subscribeEvents(callback: (err: Error | null, payloadJson: string) => void): void;
  fleetOverviewJson(): string;
  hostDetailJson(hostId: string): string;
  agentTimelineJson(hostId: string, agentId: string): string;
  gitReviewJson(hostId: string, cwd: string, path?: string | null): string;
  terminalScreenJson(hostId: string): string;
  pairHostJson(pairingUri: string): string;
  removeHost(hostId: string): void;
  updateHostRelay(hostId: string, relayUrl?: string | null): void;
  connectAllHosts(): void;
  connectHost(hostId: string): void;
  refreshHost(hostId: string): void;
  refreshQuotas(hostId: string): void;
  createAgent(
    hostId: string,
    cwd: string,
    provider: string,
    mode: string,
    initialPrompt: string,
    worktreeBranch?: string | null
  ): void;
  openAgentTimeline(hostId: string, agentId: string): void;
  sendPrompt(hostId: string, agentId: string, prompt: string): void;
  interruptAgent(hostId: string, agentId: string): void;
  setSessionMode(hostId: string, agentId: string, mode: string): void;
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
  startTerminal(hostId: string, cwd: string, cols: number, rows: number): void;
  sendTerminalKeys(hostId: string, keys: string): void;
  resizeTerminal(hostId: string, cols: number, rows: number): void;
}
