import { describe, expect, it } from '@ohos/hypium';
import {
  AgentCardDto,
  AgentStatusDto,
  FleetSidebarDto,
  OrbitProjectDirectoryGroupInput,
  OrbitTerminalSlotViewState,
  OrbitWorkspaceBoardDraft,
  TerminalScreenDeltaDto,
  UpdateWorkspaceBoardCasRequestDto,
  WorkspaceBoardCasResultDto,
  WorkspaceBoardSnapshotDto,
  WorkspaceDto,
  WorkspaceFileViewDto,
  WorkspaceScriptInfoDto,
  WorktreeHandoffPreviewDto,
  applyTerminalScreenDelta,
  clipCodeForSyncHighlightPreview,
  compareUnicodeCodePoints,
  evaluateWorktreeHandoffGuards,
  formatWorkspaceScriptStatusSummary,
  formatWorkspaceScriptSummary,
  hasBoardCasConflict,
  isWorkspaceFileEditable,
  matchesGitDiscardConfirmation,
  mergeBoardCasDraft,
  mergeWorkspaceFileChunk,
  parsePeerHandleAliases,
  resolveAuthoritativeProjectWorkspaces
} from '../../../main/ets/model/OrbitTypes';
import { OrbitCoreBridge } from '../../../main/ets/model/OrbitCoreBridge';
import { OrbitHarmonyStore } from '../../../main/ets/model/OrbitHarmonyStore';
import { OrbitSidePanelTab, encodeSidePanelKeyTuple } from '../../../main/ets/model/stores/OrbitSidePanelStore';

class FakePr5CoreBridge extends OrbitCoreBridge {
  public fleetJson: string = '{"revision":1,"hosts":[],"projects":[],"total_pending_approvals":0}';
  public sidebarJson: string = '{"revision":1,"hosts":[],"projects":[],"attention_items":[],"running_items":[]}';
  public detailJsonByHost: Map<string, string> = new Map<string, string>();
  public detailAsyncCalls: string[] = [];
  public boardJsonByKey: Map<string, string> = new Map<string, string>();
  public deltaJsonBySlot: Map<string, string> = new Map<string, string>();
  public highlightCalls: string[] = [];
  public highlightResponseJson: string = '{"language":"rust","lines":[],"total_lines":0,"truncated":false}';
  public gitPushCalls: string[] = [];
  public gitPushResponseJson: string = '{"remote":"origin","branch":"feat/pr5","output":"Everything up-to-date"}';
  public gitPushShouldFail: string = '';
  public gitDiscardCalls: string[] = [];
  public gitDiscardResponseJson: string = '{"ok":true}';
  public gitDiscardShouldFail: string = '';
  public readFileRequestCalls: string[] = [];
  public readFileRequestResponseJson: string = '';
  public writeFileCalls: string[] = [];
  public writeFileResponseJson: string = '';
  public updateBoardCalls: string[] = [];
  public updateBoardCasCalls: string[] = [];
  public boardCasServerRevByKey: Map<string, number> = new Map<string, number>();
  public boardCasServerContentByKey: Map<string, string> = new Map<string, string>();
  public startTerminalSlotCalls: string[] = [];
  public previewHandoffCalls: string[] = [];
  public previewHandoffResponseJson: string = '';
  public previewHandoffShouldFail: string = '';
  public executeHandoffCalls: string[] = [];
  public executeHandoffResponseJson: string = '';
  public executeHandoffShouldFail: string = '';
  public createWorkspaceV2Calls: string[] = [];
  public archiveWorkspaceV2Calls: string[] = [];
  public archiveWorkspaceShouldFail: string = '';
  public unarchiveWorkspaceCalls: string[] = [];
  public unarchiveWorkspaceShouldFail: string = '';
  public renameWorkspaceCalls: string[] = [];
  public renameWorkspaceShouldFail: string = '';
  public requestScriptsCalls: string[] = [];
  public startScriptCalls: string[] = [];
  public stopScriptCalls: string[] = [];
  public fetchGitDiffCalls: string[] = [];
  public gitStashListResponseJson: string = '[{"stash_ref":"stash@{0}","message":"WIP on main","oid":"abc123"}]';
  public gitPrStatusResponseJson: string = '{"number":42,"title":"PR-5 Tools","state":"OPEN","url":"https://example.local/pr/42","passing_checks":3,"failing_checks":0}';
  public gitConflictsResponseJson: string = '[]';
  public gitRebaseStatusResponseJson: string = '{"in_progress":false}';

  constructor() {
    super(null, false);
  }

  public override subscribeEvents(_callback: (err: Error | null, payloadJson: string) => void): void {}
  public override fallbackProviderCatalogsJson(): string { return '[]'; }
  public override slashCommandsJson(_platform?: string | null, _query?: string | null): string { return '[]'; }
  public override cloudSyncStatusJson(): string {
    return '{"enabled":false,"configured":false,"state":"disabled","device_id":"","last_sync_at":null,"last_error":null}';
  }
  public override fleetOverviewJson(): string { return this.fleetJson; }
  public override async fleetOverviewJsonAsync(): Promise<string> { return this.fleetJson; }
  public override fleetSidebarByRequestJson(_reqJson: string): string { return this.sidebarJson; }
  public override async fleetSidebarByRequestJsonAsync(_reqJson: string): Promise<string> { return this.sidebarJson; }
  public override hostDetailJson(hostId: string): string { return this.detailJsonByHost.get(hostId) ?? 'null'; }
  public override async hostDetailJsonAsync(hostId: string): Promise<string> {
    this.detailAsyncCalls.push(hostId);
    return this.detailJsonByHost.get(hostId) ?? 'null';
  }
  public override connectAllHosts(): void {}
  public override connectHost(_hostId: string): void {}
  public override refreshHost(_hostId: string): void {}
  public override refreshQuotas(_hostId: string): void {}
  public override refreshProviderModels(_hostId: string, _forceRefresh: boolean): void {}
  public override fetchGitStatus(_hostId: string, _cwd: string): void {}
  public override fetchGitDiff(hostId: string, cwd: string, path: string | null | undefined, staged: boolean): void {
    this.fetchGitDiffCalls.push(`${hostId}|${cwd}|${path ?? '__all__'}|${staged ? 'S' : 'U'}`);
  }
  public override gitReviewJson(_hostId: string, _cwd: string, _path?: string | null, _staged?: boolean | null): string {
    return '{"revision":1,"host_id":"h1","cwd":"/repo","status":null,"selected_path":null,"staged":false,"diff_revision":1,"raw_diff":"","diff_files":[]}';
  }
  public override async gitPushJson(reqJson: string): Promise<string> {
    this.gitPushCalls.push(reqJson);
    if (this.gitPushShouldFail.length > 0) {
      throw new Error(this.gitPushShouldFail);
    }
    return this.gitPushResponseJson;
  }
  public override async gitDiscardFilesJson(reqJson: string): Promise<string> {
    this.gitDiscardCalls.push(reqJson);
    if (this.gitDiscardShouldFail.length > 0) {
      throw new Error(this.gitDiscardShouldFail);
    }
    return this.gitDiscardResponseJson;
  }
  public override async gitPrStatusJson(_hostId: string, _cwd: string): Promise<string> {
    return this.gitPrStatusResponseJson;
  }
  public override async gitStashListJson(_hostId: string, _cwd: string): Promise<string> {
    return this.gitStashListResponseJson;
  }
  public override async gitConflictsJson(_hostId: string, _cwd: string): Promise<string> {
    return this.gitConflictsResponseJson;
  }
  public override async gitRebaseStatusJson(_hostId: string, _cwd: string): Promise<string> {
    return this.gitRebaseStatusResponseJson;
  }
  public override highlightWorkspaceCodeJson(reqJson: string): string {
    this.highlightCalls.push(reqJson);
    return this.highlightResponseJson;
  }
  public override async readWorkspaceFileRequestJson(reqJson: string): Promise<string> {
    this.readFileRequestCalls.push(reqJson);
    return this.readFileRequestResponseJson;
  }
  public override async writeWorkspaceFileJson(reqJson: string): Promise<string> {
    this.writeFileCalls.push(reqJson);
    return this.writeFileResponseJson;
  }
  public override terminalScreenDeltaSlotJson(hostId: string, slot: number, sinceRevision: number): string {
    const key = `${hostId}|${slot}|${sinceRevision}`;
    const exact = this.deltaJsonBySlot.get(key);
    if (exact !== undefined) {
      return exact;
    }
    const fallback = this.deltaJsonBySlot.get(`${hostId}|${slot}`);
    return fallback !== undefined ? fallback : 'null';
  }
  public override startTerminalSlot(hostId: string, cwd: string, cols: number, rows: number, slot: number): void {
    this.startTerminalSlotCalls.push(`${hostId}|${cwd}|${cols}x${rows}|slot=${slot}`);
  }
  public override workspaceBoardJson(hostId: string, workspaceId: string): string {
    return this.boardJsonByKey.get(`${hostId}|${workspaceId}`) ?? 'null';
  }
  public override updateWorkspaceBoard(
    hostId: string,
    workspaceId: string,
    content: string,
    _section: string | null,
    _updatedBy: string | null
  ): void {
    this.updateBoardCalls.push(`${hostId}|${workspaceId}|${content}`);
  }
  public override async updateWorkspaceBoardCasJson(reqJson: string): Promise<string> {
    this.updateBoardCasCalls.push(reqJson);
    const req = JSON.parse(reqJson) as UpdateWorkspaceBoardCasRequestDto;
    const hostId = req.host_id;
    const wsId = req.workspace_id;
    const content = req.content;
    const key = `${hostId}|${wsId}`;
    const liveRev = this.boardCasServerRevByKey.get(key) ?? 0;
    const liveContent = this.boardCasServerContentByKey.get(key) ?? '';
    const hasExp = req.expected_revision !== undefined && req.expected_revision !== null;
    const expRev = hasExp ? (req.expected_revision as number) : -1;
    if (hasExp && expRev !== liveRev) {
      const conflictBoard: WorkspaceBoardSnapshotDto = {
        workspace_id: wsId,
        content: liveContent,
        revision: liveRev,
        updated_by: 'remote-peer',
        updated_at: '2026-11-27T00:00:00Z',
        board_path: '/repo/BOARD.md'
      };
      const conflictOut: WorkspaceBoardCasResultDto = {
        ok: false,
        conflict: true,
        board: conflictBoard
      };
      return JSON.stringify(conflictOut);
    }
    const nextRev = liveRev + 1;
    this.boardCasServerRevByKey.set(key, nextRev);
    this.boardCasServerContentByKey.set(key, content);
    const updatedBoard: WorkspaceBoardSnapshotDto = {
      workspace_id: wsId,
      content: content,
      revision: nextRev,
      updated_by: 'harmony-user',
      updated_at: '2026-11-27T00:00:00Z',
      board_path: '/repo/BOARD.md'
    };
    this.boardJsonByKey.set(key, JSON.stringify(updatedBoard));
    const okOut: WorkspaceBoardCasResultDto = {
      ok: true,
      conflict: false,
      board: updatedBoard
    };
    return JSON.stringify(okOut);
  }
  public override async previewWorktreeHandoffJson(reqJson: string): Promise<string> {
    this.previewHandoffCalls.push(reqJson);
    if (this.previewHandoffShouldFail.length > 0) {
      throw new Error(this.previewHandoffShouldFail);
    }
    return this.previewHandoffResponseJson;
  }
  public override async executeWorktreeHandoffJson(reqJson: string): Promise<string> {
    this.executeHandoffCalls.push(reqJson);
    if (this.executeHandoffShouldFail.length > 0) {
      throw new Error(this.executeHandoffShouldFail);
    }
    return this.executeHandoffResponseJson;
  }
  public override createWorkspaceV2Json(reqJson: string): void {
    this.createWorkspaceV2Calls.push(reqJson);
  }
  public override async archiveWorkspaceV2Json(
    hostId: string,
    workspaceId: string,
    keepAgentsUnclassified: boolean
  ): Promise<string> {
    this.archiveWorkspaceV2Calls.push(`${hostId}|${workspaceId}|${keepAgentsUnclassified ? 'keep' : 'archive'}`);
    if (this.archiveWorkspaceShouldFail.length > 0) {
      throw new Error(this.archiveWorkspaceShouldFail);
    }
    return '{"ok":true}';
  }
  public override async unarchiveWorkspaceJson(hostId: string, workspaceId: string): Promise<string> {
    this.unarchiveWorkspaceCalls.push(`${hostId}|${workspaceId}`);
    if (this.unarchiveWorkspaceShouldFail.length > 0) {
      throw new Error(this.unarchiveWorkspaceShouldFail);
    }
    return '{"ok":true}';
  }
  public override async renameWorkspaceConfirmedJson(
    hostId: string,
    workspaceId: string,
    title: string
  ): Promise<string> {
    this.renameWorkspaceCalls.push(`${hostId}|${workspaceId}|${title}`);
    if (this.renameWorkspaceShouldFail.length > 0) {
      throw new Error(this.renameWorkspaceShouldFail);
    }
    return '{"ok":true}';
  }
  public override requestWorkspaceScripts(hostId: string, workspaceId: string): void {
    this.requestScriptsCalls.push(`${hostId}|${workspaceId}`);
  }
  public override startWorkspaceScriptJson(hostId: string, workspaceId: string, scriptName: string): void {
    this.startScriptCalls.push(`${hostId}|${workspaceId}|${scriptName}`);
  }
  public override stopWorkspaceScriptJson(hostId: string, workspaceId: string, scriptName: string): void {
    this.stopScriptCalls.push(`${hostId}|${workspaceId}|${scriptName}`);
  }
}

function makePr5HostDetailJson(
  hostId: string,
  workspacesJson: string[],
  agentsJson: string[],
  scriptsMapJson: string = '{}',
  boardCasSupported: boolean = true
): string {
  return `{"revision":5,"host":{"host_id":"${hostId}","display_name":"${hostId}","hostname":"${hostId}","custom_name":null,"ssh_alias":null,"route_mode":"auto","remote_daemon_port":6868,"custom_direct_endpoint":null,"via_ssh_tunnel":false,"last_ping_ms":8,"daemon_version":"1.0","os_arch":"linux","daemon_pubkey_b64":"","direct_endpoints":[],"relay_url":null,"connection_state":"online_direct","last_error":null,"available_providers":["claude"],"workspace_count":${workspacesJson.length},"agent_count":${agentsJson.length},"pending_approval_count":0,"board_cas_supported":${boardCasSupported},"is_local":false},"agents":[${agentsJson.join(',')}],"workspaces":[${workspacesJson.join(',')}],"terminals":[],"active_terminal_id":null,"active_terminal_slot":1,"provider_catalogs":[],"models_by_provider":{"antigravity":[],"codex":[],"claude":[],"pi":[],"acp":[]},"discovered_projects":[],"directory_listing":null,"quotas":[],"last_created_agent_id":null,"workspace_scripts":${scriptsMapJson}}`;
}

function makePr5AgentCard(
  agentId: string,
  cwd: string,
  workspaceId: string,
  handle: string | null = null,
  aliases: string = ''
): AgentCardDto {
  const labels: Record<string, string> = {};
  if (aliases.length > 0) {
    labels['orbit.handle_aliases'] = aliases;
  }
  const statusObj: AgentStatusDto = { state: 'idle' };
  const card: AgentCardDto = {
    agent_id: agentId,
    workspace_id: workspaceId,
    cwd: cwd,
    title: agentId,
    provider: 'claude',
    provider_label: 'Claude',
    model: 'claude-sonnet',
    effort: null,
    mode: 'auto',
    status: statusObj,
    status_label: 'Idle',
    is_running: false,
    is_awaiting_permission: false,
    parent_agent_id: null,
    latest_seq: 1,
    updated_at_rfc3339: '2026-11-27T00:00:00Z',
    handle: handle,
    labels: labels
  };
  return card;
}

export default function orbitPr5Test(): void {
  describe('OrbitPr5', () => {
    it('terminalDeltaUpdatesDirtyRowsResetAndMultiSlotIsolation', 0, () => {
      const resetDelta: TerminalScreenDeltaDto = {
        revision: 1,
        slot: 1,
        reset: true,
        cols: 80,
        rows: 4,
        cursor_row: 0,
        cursor_col: 5,
        changed_rows: [
          {
            row_index: 0,
            spans: [{ text: 'prompt$', fg_hex: '#E2E8F0', bg_hex: null, bold: false, underline: false, inverse: false }]
          },
          { row_index: 1, spans: [] },
          { row_index: 2, spans: [] },
          { row_index: 3, spans: [] }
        ]
      };
      const initRes = applyTerminalScreenDelta(null, 0, resetDelta);
      expect(initRes.revision).assertEqual(1);
      expect(initRes.screen.rows_data.length).assertEqual(4);
      expect(initRes.screen.rows_data[0].spans[0].text).assertEqual('prompt$');

      // Incremental dirty-row update touching only 2 rows (simulating `ls`)
      const lsDelta: TerminalScreenDeltaDto = {
        revision: 2,
        slot: 1,
        reset: false,
        cols: 80,
        rows: 4,
        cursor_row: 2,
        cursor_col: 0,
        changed_rows: [
          {
            row_index: 1,
            spans: [{ text: 'Cargo.toml README.md', fg_hex: '#4ADE80', bg_hex: null, bold: false, underline: false, inverse: false }]
          },
          {
            row_index: 2,
            spans: [{ text: 'apps crates', fg_hex: '#60A5FA', bg_hex: null, bold: true, underline: false, inverse: false }]
          }
        ]
      };
      const lsRes = applyTerminalScreenDelta(initRes.screen, initRes.revision, lsDelta);
      expect(lsRes.revision).assertEqual(2);
      expect(lsRes.updatedRowCount).assertEqual(2);
      expect(lsRes.screen.rows_data[0].spans[0].text).assertEqual('prompt$');
      expect(lsRes.screen.rows_data[1].spans[0].text).assertEqual('Cargo.toml README.md');
      expect(lsRes.screen.rows_data[2].spans[0].text).assertEqual('apps crates');

      // Same revision no-op returns 0 updated rows
      const noopRes = applyTerminalScreenDelta(lsRes.screen, lsRes.revision, {
        revision: 2,
        slot: 1,
        reset: false,
        cols: 80,
        rows: 4,
        cursor_row: 2,
        cursor_col: 0,
        changed_rows: []
      });
      expect(noopRes.updatedRowCount).assertEqual(0);
      expect(noopRes.screen === lsRes.screen).assertTrue();

      // Resize/reset rebuilds full grid with new dimensions
      const resizeRes = applyTerminalScreenDelta(lsRes.screen, lsRes.revision, {
        revision: 3,
        slot: 1,
        reset: true,
        cols: 120,
        rows: 6,
        cursor_row: 0,
        cursor_col: 0,
        changed_rows: []
      });
      expect(resizeRes.screen.cols).assertEqual(120);
      expect(resizeRes.screen.rows).assertEqual(6);
      expect(resizeRes.screen.rows_data.length).assertEqual(6);

      // Multi-slot 1..3 isolation + explicit target slot on startScopedTerminal + scope-explicit slot persistence (P0-3)
      const bridge = new FakePr5CoreBridge();
      bridge.deltaJsonBySlot.set('h1|1|0', '{"revision":10,"slot":1,"reset":true,"cols":80,"rows":2,"cursor_row":0,"cursor_col":0,"changed_rows":[{"row_index":0,"spans":[{"text":"slot-1","fg_hex":"#E2E8F0","bg_hex":null,"bold":false,"underline":false,"inverse":false}]}]}');
      bridge.deltaJsonBySlot.set('h1|2|0', '{"revision":20,"slot":2,"reset":true,"cols":80,"rows":2,"cursor_row":0,"cursor_col":0,"changed_rows":[{"row_index":0,"spans":[{"text":"slot-2","fg_hex":"#E2E8F0","bg_hex":null,"bold":false,"underline":false,"inverse":false}]}]}');
      bridge.deltaJsonBySlot.set('h1|3|0', '{"revision":30,"slot":3,"reset":true,"cols":80,"rows":2,"cursor_row":0,"cursor_col":0,"changed_rows":[{"row_index":0,"spans":[{"text":"slot-3","fg_hex":"#E2E8F0","bg_hex":null,"bold":false,"underline":false,"inverse":false}]}]}');

      const store = new OrbitHarmonyStore();
      store.initializeWithTestBridge(bridge);
      const d1 = store.loadScopedTerminalDelta('h1', 1, 0);
      const d2 = store.loadScopedTerminalDelta('h1', 2, 0);
      const d3 = store.loadScopedTerminalDelta('h1', 3, 0);
      const d0 = store.loadScopedTerminalDelta('h1', 0, 0);
      expect(d1?.revision).assertEqual(10);
      expect(d2?.revision).assertEqual(20);
      expect(d3?.revision).assertEqual(30);
      expect(d0?.slot).assertEqual(1);
      expect(d2?.changed_rows[0].spans[0].text).assertEqual('slot-2');
      expect(d3?.changed_rows[0].spans[0].text).assertEqual('slot-3');

      // P0-3: New Terminal explicitly passes target slot
      store.startScopedTerminal('h1', '/repo', 100, 32, 2);
      store.startScopedTerminal('h1', '/repo', 80, 24, 3);
      expect(bridge.startTerminalSlotCalls.length).assertEqual(2);
      expect(bridge.startTerminalSlotCalls[0]).assertEqual('h1|/repo|100x32|slot=2');
      expect(bridge.startTerminalSlotCalls[1]).assertEqual('h1|/repo|80x24|slot=3');

      // P0-3: Side panel slot persistence updates explicit scopeKey even when activeScopeKey differs
      store.sidePanelStore.switchScope('h1', 'ag1');
      const termTab1: OrbitSidePanelTab = {
        tabId: 'term:h1:/repo',
        kind: 'terminal',
        title: 'Shell',
        hostId: 'h1',
        parentHostId: null,
        agentId: 'ag1',
        cwd: '/repo',
        path: null,
        closable: true,
        terminalSlot: 1
      };
      store.sidePanelStore.openTab(termTab1, { source: 'user', activate: true });
      const tabId1 = store.sidePanelStore.activeTabId;
      store.sidePanelStore.switchScope('h1', 'ag2');
      const termTab2: OrbitSidePanelTab = {
        tabId: 'term:h1:/repo2',
        kind: 'terminal',
        title: 'Shell 2',
        hostId: 'h1',
        parentHostId: null,
        agentId: 'ag2',
        cwd: '/repo2',
        path: null,
        closable: true,
        terminalSlot: 1
      };
      store.sidePanelStore.openTab(termTab2, { source: 'user', activate: true });
      const tabId2 = store.sidePanelStore.activeTabId;
      // Update slot for h1::ag1 while h1::ag2 is active
      store.sidePanelStore.updateTabTerminalSlot('h1::ag1', 'h1', tabId1, 2);
      expect(store.sidePanelStore.activeTab?.tabId).assertEqual(tabId2);
      expect(store.sidePanelStore.activeTab?.terminalSlot ?? 1).assertEqual(1);
      store.sidePanelStore.switchScope('h1', 'ag1');
      expect(store.sidePanelStore.activeTab?.terminalSlot).assertEqual(2);

      // P0-3: Per-(scopeKey, hostId, tabId, slot) state map strictly isolates drafts, modes, and sticky modifiers
      const slotStates = new Map<string, OrbitTerminalSlotViewState>();
      const k1 = encodeSidePanelKeyTuple(['pty_slot', 'h1::ag1', 'h1', tabId1, '1']);
      const k2 = encodeSidePanelKeyTuple(['pty_slot', 'h1::ag1', 'h1', tabId1, '2']);
      slotStates.set(k1, {
        screen: null,
        revision: 10,
        lastRequestedRevision: 10,
        lineInput: 'echo slot-one',
        rawMode: false,
        ctrlSticky: true,
        altSticky: false
      });
      slotStates.set(k2, {
        screen: null,
        revision: 20,
        lastRequestedRevision: 20,
        lineInput: '',
        rawMode: true,
        ctrlSticky: false,
        altSticky: true
      });
      expect(slotStates.get(k2)?.lineInput).assertEqual('');
      expect(slotStates.get(k2)?.rawMode).assertTrue();
      expect(slotStates.get(k2)?.ctrlSticky).assertFalse();
      expect(slotStates.get(k1)?.lineInput).assertEqual('echo slot-one');
      expect(slotStates.get(k1)?.ctrlSticky).assertTrue();
      store.sidePanelStore.dispose();
    });

    it('gitDiscardRequiresExactFilenameAndPushCollabAreSilentOnStaleHost', 0, async () => {
      // Two-step Discard filename confirmation guard
      expect(matchesGitDiscardConfirmation('src/main.rs', '')).assertFalse();
      expect(matchesGitDiscardConfirmation('src/main.rs', 'main')).assertFalse();
      expect(matchesGitDiscardConfirmation('src/main.rs', 'other.rs')).assertFalse();
      expect(matchesGitDiscardConfirmation('src/main.rs', 'main.rs')).assertTrue();
      expect(matchesGitDiscardConfirmation('src/main.rs', 'src/main.rs')).assertTrue();
      // Literal trailing space preserved strictly
      expect(matchesGitDiscardConfirmation('docs/notes .txt ', 'notes .txt')).assertFalse();
      expect(matchesGitDiscardConfirmation('docs/notes .txt ', 'notes .txt ')).assertTrue();
      expect(matchesGitDiscardConfirmation('docs/notes .txt ', 'docs/notes .txt ')).assertTrue();

      const bridge = new FakePr5CoreBridge();
      const store = new OrbitHarmonyStore();
      store.initializeWithTestBridge(bridge);

      const toastRevBefore = store.toastRevision;
      const lastErrBefore = store.lastError;

      // Push with empty remote normalizes to null (default remote) AND does NOT mutate global toast/lastError (P1-5)
      const pushRes = await store.gitPushScoped('h1', '/repo', '   ', '', true);
      expect(pushRes.ok).assertTrue();
      expect(pushRes.data?.remote).assertEqual('origin');
      expect(pushRes.data?.branch).assertEqual('feat/pr5');
      expect(bridge.gitPushCalls.length).assertEqual(1);
      expect(bridge.gitPushCalls[0].indexOf('"remote":null') >= 0).assertTrue();
      expect(bridge.gitPushCalls[0].indexOf('"set_upstream":true') >= 0).assertTrue();
      expect(store.toastRevision).assertEqual(toastRevBefore);
      expect(store.lastError).assertEqual(lastErrBefore);

      // Late failure on h1 also does NOT pollute global toast or lastError
      bridge.gitPushShouldFail = 'h1 offline';
      const pushErr = await store.gitPushScoped('h1', '/repo', 'origin', 'main', false);
      expect(pushErr.ok).assertFalse();
      expect(pushErr.errorDetail).assertEqual('h1 offline');
      expect(store.toastRevision).assertEqual(toastRevBefore);
      expect(store.lastError).assertEqual(lastErrBefore);
      bridge.gitPushShouldFail = '';

      // Discard preserves literal trailing space path, returns structured outcome, stays silent on global toast (P1-5)
      const discarded = await store.gitDiscardScopedFiles('h1', '/repo', ['docs/notes .txt '], true);
      expect(discarded.ok).assertTrue();
      expect(discarded.data).assertEqual(1);
      expect(bridge.gitDiscardCalls.length).assertEqual(1);
      expect(bridge.gitDiscardCalls[0].indexOf('"docs/notes .txt "') >= 0).assertTrue();
      expect(bridge.gitDiscardCalls[0].indexOf('"include_untracked":true') >= 0).assertTrue();
      expect(store.toastRevision).assertEqual(toastRevBefore);

      // Collab status returns stashes, PR, conflicts, rebase
      const collab = await store.loadScopedGitCollabStatus('h1', '/repo');
      expect(collab.stashes.length).assertEqual(1);
      expect(collab.stashes[0].stash_ref).assertEqual('stash@{0}');
      expect(collab.prStatus?.number).assertEqual(42);
      expect(collab.rebaseStatus?.in_progress).assertFalse();
      store.sidePanelStore.dispose();
    });

    it('filesHighlightCapsAt250LinesChunkedReadSupportsEtagAndKeepsTruncatedReadOnly', 0, async () => {
      const bridge = new FakePr5CoreBridge();
      bridge.highlightResponseJson = '{"language":"rust","lines":[{"line_number":1,"tokens":[{"text":"fn","fg_hex":"#C084FC","light_fg_hex":"#7C3AED","bold":true,"italic":false}]}],"total_lines":420,"truncated":true}';
      const store = new OrbitHarmonyStore();
      store.initializeWithTestBridge(bridge);

      const highlighted = store.highlightScopedWorkspaceCode('rust', 'fn main() {}', 500);
      expect(highlighted !== null).assertTrue();
      expect(highlighted?.truncated).assertTrue();
      expect(highlighted?.total_lines).assertEqual(420);
      expect(highlighted?.lines[0].tokens[0].light_fg_hex).assertEqual('#7C3AED');
      expect(bridge.highlightCalls.length).assertEqual(1);
      expect(bridge.highlightCalls[0].indexOf('"max_lines":250') >= 0).assertTrue();

      // P1-3: Truncated source file remains strictly non-editable and loads tail via ETag-bound chunked read
      const truncView: WorkspaceFileViewDto = {
        host_id: 'h1',
        cwd: '/repo',
        path: 'big.log',
        resolved_path: '/repo/big.log',
        language: 'text',
        content_kind: 'text',
        mime_type: 'text/plain',
        content: 'prefix-2mib',
        size_bytes: 2400010,
        truncated: true,
        etag: 'sha256-v1',
        writable: true,
        offset_bytes: 0,
        next_offset_bytes: 2097152,
        etag_mismatch: false,
        highlighted_lines: [],
        markdown_blocks: []
      };
      expect(isWorkspaceFileEditable(truncView)).assertFalse();

      const chunkOkView: WorkspaceFileViewDto = {
        host_id: 'h1',
        cwd: '/repo',
        path: 'big.log',
        resolved_path: '/repo/big.log',
        language: 'text',
        content_kind: 'text',
        mime_type: 'text/plain',
        content: '\nEND_MARKER_EOF\n',
        size_bytes: 2400010,
        truncated: false,
        etag: 'sha256-v1',
        writable: true,
        offset_bytes: 2097152,
        next_offset_bytes: 2400010,
        etag_mismatch: false,
        highlighted_lines: [],
        markdown_blocks: []
      };
      bridge.readFileRequestResponseJson = JSON.stringify(chunkOkView);
      const chunkRes = await store.readScopedWorkspaceFileChunk({
        host_id: 'h1',
        cwd: '/repo',
        path: 'big.log',
        offset_bytes: 2097152,
        max_bytes: 2097152,
        expected_etag: 'sha256-v1'
      });
      expect(chunkRes.ok).assertTrue();
      expect(chunkRes.data?.truncated).assertFalse();
      expect(chunkRes.data?.etag_mismatch).assertFalse();
      expect((chunkRes.data?.content ?? '').indexOf('END_MARKER_EOF') >= 0).assertTrue();
      expect(bridge.readFileRequestCalls[0].indexOf('"expected_etag":"sha256-v1"') >= 0).assertTrue();

      // P1-3: ETag mismatch surfaces cleanly without corrupting content
      const chunkMismatchView: WorkspaceFileViewDto = {
        host_id: 'h1',
        cwd: '/repo',
        path: 'big.log',
        resolved_path: '/repo/big.log',
        language: 'text',
        content_kind: 'text',
        mime_type: 'text/plain',
        content: '',
        size_bytes: 2400100,
        truncated: true,
        etag: 'sha256-v2',
        writable: true,
        offset_bytes: 2097152,
        next_offset_bytes: 2097152,
        etag_mismatch: true,
        highlighted_lines: [],
        markdown_blocks: []
      };
      bridge.readFileRequestResponseJson = JSON.stringify(chunkMismatchView);
      const mismatchRes = await store.readScopedWorkspaceFileChunk({
        host_id: 'h1',
        cwd: '/repo',
        path: 'big.log',
        offset_bytes: 2097152,
        max_bytes: 2097152,
        expected_etag: 'sha256-v1'
      });
      expect(mismatchRes.ok).assertTrue();
      expect(mismatchRes.data?.etag_mismatch).assertTrue();
      expect(mismatchRes.data?.content).assertEqual('');

      // ETag conflict on write surfaces conflict=true on force=false, then succeeds on force=true
      bridge.writeFileResponseJson = '{"host_id":"h1","cwd":"/repo","path":"src/lib.rs","size_bytes":24,"etag":"etag-v2","conflict":{"current_etag":"etag-v2","current_size_bytes":24,"current_content":"remote"}}';
      const conflictRes = await store.saveScopedWorkspaceFile('h1', '/repo', 'src/lib.rs', 'local', 'etag-v1', false);
      expect(conflictRes.ok).assertFalse();
      expect(conflictRes.conflict).assertTrue();
      expect(conflictRes.etag).assertEqual('etag-v2');

      bridge.writeFileResponseJson = '{"host_id":"h1","cwd":"/repo","path":"src/lib.rs","size_bytes":5,"etag":"etag-v3","conflict":null}';
      const forceRes = await store.saveScopedWorkspaceFile('h1', '/repo', 'src/lib.rs', 'local', null, true);
      expect(forceRes.ok).assertTrue();
      expect(forceRes.conflict).assertFalse();
      store.sidePanelStore.dispose();
    });

    it('boardServerSideCasHandlesRev0ConcurrentWritersDraftBaseAndPeerAliases', 0, async () => {
      // P0-1: rev 0 vs rev 5 MUST be a CAS conflict (never treated as falsy)
      expect(hasBoardCasConflict(0, 0)).assertFalse();
      expect(hasBoardCasConflict(0, 5)).assertTrue();
      expect(hasBoardCasConflict(3, 3)).assertFalse();
      expect(hasBoardCasConflict(3, 5)).assertTrue();
      const merged = mergeBoardCasDraft('## Remote', '## Local');
      expect(merged.indexOf('## Local') >= 0).assertTrue();
      expect(merged.indexOf('## Remote') >= 0).assertTrue();

      // P1-8: Peer historical aliases parsing + cached projection deduplication
      const peer = makePr5AgentCard('ag-peer', '/repo', 'ws-1', 'reviewer', '@old-rev, reviewer, old-rev, qa-lead');
      const aliases = parsePeerHandleAliases(peer);
      expect(aliases.length).assertEqual(2);
      expect(aliases[0]).assertEqual('old-rev');
      expect(aliases[1]).assertEqual('qa-lead');

      const bridge = new FakePr5CoreBridge();
      bridge.detailJsonByHost.set(
        'h1',
        makePr5HostDetailJson(
          'h1',
          ['{"workspace_id":"ws-1","title":"Main WS","cwd":"/repo","git_branch":"main","is_worktree":false,"board_revision":0}'],
          ['{"agent_id":"ag-1","host_id":"h1","workspace_id":"ws-1","title":"Lead","provider":"claude","provider_label":"Claude","model":"claude-sonnet","mode":"auto","cwd":"/repo","status":{"state":"idle"},"is_running":false,"is_awaiting_permission":false,"has_pending_question":false,"latest_seq":1,"updated_at_rfc3339":"2026-11-27T00:00:00Z"}']
        )
      );
      bridge.boardCasServerRevByKey.set('h1|ws-1', 0);
      bridge.boardCasServerContentByKey.set('h1|ws-1', '');
      bridge.boardJsonByKey.set('h1|ws-1', '{"workspace_id":"ws-1","content":"","revision":0,"updated_by":null,"board_path":"/repo/BOARD.md"}');

      const store = new OrbitHarmonyStore();
      store.initializeWithTestBridge(bridge);
      await store.fleetStore.reloadHostAsync('h1');

      const loaded = store.loadScopedWorkspaceBoard('h1', '/repo', 'ws-1', 'ag-1', false);
      expect(loaded.workspaceVerified).assertTrue();
      expect(loaded.snapshotLoaded).assertTrue();
      expect(loaded.revision).assertEqual(0);

      // P0-1: Two concurrent saves from rev 0 -> first succeeds (rev 1), second gets server CAS conflict!
      const saveAlpha = await store.saveScopedWorkspaceBoardWithCas('h1', 'ws-1', 'alpha', 0, false, '/repo', 'ag-1');
      expect(saveAlpha.ok).assertTrue();
      expect(saveAlpha.conflict).assertFalse();
      expect(saveAlpha.currentRevision).assertEqual(1);
      expect(bridge.updateBoardCasCalls[0].indexOf('"expected_revision":0') >= 0).assertTrue();

      const saveBeta = await store.saveScopedWorkspaceBoardWithCas('h1', 'ws-1', 'beta', 0, false, '/repo', 'ag-1');
      expect(saveBeta.ok).assertFalse();
      expect(saveBeta.conflict).assertTrue();
      expect(saveBeta.currentRevision).assertEqual(1);
      expect(saveBeta.currentContent).assertEqual('alpha');

      // P0-1: Draft preserves original baseRevision even after remote advances to rev 7
      const draftStore = new Map<string, OrbitWorkspaceBoardDraft>();
      draftStore.set('h1|ws-1', { content: 'draft from v5', baseRevision: 5 });
      const restored = draftStore.get('h1|ws-1');
      expect(restored?.content).assertEqual('draft from v5');
      expect(restored?.baseRevision).assertEqual(5);

      // Explicit force overwrite passes expected_revision: null and succeeds
      const forceSave = await store.saveScopedWorkspaceBoardWithCas('h1', 'ws-1', '# Forced content', 0, true, '/repo', 'ag-1');
      expect(forceSave.ok).assertTrue();
      expect(forceSave.conflict).assertFalse();
      expect(forceSave.currentRevision).assertEqual(2);
      const lastCall = bridge.updateBoardCasCalls[bridge.updateBoardCasCalls.length - 1];
      expect(lastCall.indexOf('"expected_revision":null') >= 0).assertTrue();

      // Missing snapshot still fails closed even with force=true
      bridge.boardJsonByKey.set('h1|ws-1', 'null');
      const noSnapSave = await store.saveScopedWorkspaceBoardWithCas('h1', 'ws-1', '# Blocked', 2, true, '/repo', 'ag-1');
      expect(noSnapSave.ok).assertFalse();
      expect(noSnapSave.errorCode).assertEqual('snapshot_not_loaded');
      store.sidePanelStore.dispose();
    });

    it('worktreeHandoffGuardsMatchDesktopAndStaySilentOnStaleErrors', 0, async () => {
      // P1-4: Hard blocker (`can_apply_cleanly: false, conflicts: []`) must block execution
      const hardBlockedPreview: WorktreeHandoffPreviewDto = {
        direction: 'worktree_to_local',
        source_cwd: '/repo-wt',
        target_cwd: '/repo',
        commit_count: 1,
        files: [{ path: 'src/a.ts', status: 'M', insertions: 4, deletions: 1, has_conflict: false, is_untracked: false }],
        total_insertions: 4,
        total_deletions: 1,
        target_dirty: true,
        conflicts: [],
        can_apply_cleanly: false,
        warning: 'Target checkout has conflicting dirty changes'
      };
      const hardGuard = evaluateWorktreeHandoffGuards(hardBlockedPreview, 'worktree_to_local', null, true, false, false);
      expect(hardGuard.hasHardBlocker).assertTrue();
      expect(hardGuard.canExecute).assertFalse();
      expect(hardGuard.canOfferThreeWay).assertFalse();

      // P1-4: Untracked file conflict must NEVER allow execution even with allowThreeWay=true
      const untrackedConflictPreview: WorktreeHandoffPreviewDto = {
        direction: 'worktree_to_local',
        source_cwd: '/repo-wt',
        target_cwd: '/repo',
        commit_count: 0,
        files: [{ path: 'new-file.ts', status: 'A', insertions: 10, deletions: 0, has_conflict: true, is_untracked: true }],
        total_insertions: 10,
        total_deletions: 0,
        target_dirty: true,
        conflicts: ['new-file.ts'],
        can_apply_cleanly: false
      };
      const untrackedGuard = evaluateWorktreeHandoffGuards(untrackedConflictPreview, 'worktree_to_local', null, true, false, false);
      expect(untrackedGuard.hasUntrackedConflict).assertTrue();
      expect(untrackedGuard.canOfferThreeWay).assertFalse();
      expect(untrackedGuard.canExecute).assertFalse();

      // Tracked conflict allows 3-way opt-in
      const trackedConflictPreview: WorktreeHandoffPreviewDto = {
        direction: 'worktree_to_local',
        source_cwd: '/repo-wt',
        target_cwd: '/repo',
        commit_count: 1,
        files: [{ path: 'src/b.ts', status: 'M', insertions: 2, deletions: 2, has_conflict: true, is_untracked: false }],
        total_insertions: 2,
        total_deletions: 2,
        target_dirty: false,
        conflicts: ['src/b.ts'],
        can_apply_cleanly: false
      };
      expect(evaluateWorktreeHandoffGuards(trackedConflictPreview, 'worktree_to_local', null, false, false, false).canExecute).assertFalse();
      const threeWayGuard = evaluateWorktreeHandoffGuards(trackedConflictPreview, 'worktree_to_local', null, true, false, false);
      expect(threeWayGuard.canOfferThreeWay).assertTrue();
      expect(threeWayGuard.canExecute).assertTrue();
      // Executing freezes execution button
      expect(evaluateWorktreeHandoffGuards(trackedConflictPreview, 'worktree_to_local', null, true, false, true).canExecute).assertFalse();

      const bridge = new FakePr5CoreBridge();
      bridge.previewHandoffResponseJson = '{"direction":"worktree_to_local","source_cwd":"/repo-wt","target_cwd":"/repo","commit_count":1,"files":[{"path":"src/app.ts","status":"M","insertions":12,"deletions":3,"has_conflict":false}],"total_insertions":12,"total_deletions":3,"target_dirty":false,"conflicts":[],"can_apply_cleanly":true}';
      bridge.executeHandoffResponseJson = '{"direction":"worktree_to_local","mode":"apply","applied_files":1,"backup_ref":"refs/orbit/handoff-backup/1","target_cwd":"/repo","had_conflicts":false,"conflict_files":[],"summary":"Applied 1 file cleanly"}';

      const store = new OrbitHarmonyStore();
      store.initializeWithTestBridge(bridge);
      const toastBefore = store.toastRevision;

      const previewRes = await store.previewScopedWorktreeHandoff({
        host_id: 'h-wt-a',
        workspace_id: 'ws-wt-a',
        source_agent_id: 'ag-wt-a',
        direction: 'worktree_to_local',
        target_workspace_id: null,
        include_committed: true
      });
      expect(previewRes.ok).assertTrue();
      expect(previewRes.data?.can_apply_cleanly).assertTrue();
      expect(previewRes.data?.files.length).assertEqual(1);
      expect(bridge.previewHandoffCalls[0].indexOf('"workspace_id":"ws-wt-a"') >= 0).assertTrue();

      const outcomeRes = await store.executeScopedWorktreeHandoff({
        host_id: 'h-wt-a',
        workspace_id: 'ws-wt-a',
        source_agent_id: 'ag-wt-a',
        direction: 'worktree_to_local',
        mode: 'apply',
        target_workspace_id: null,
        include_committed: true,
        branch: null,
        allow_three_way_conflicts: false
      });
      expect(outcomeRes.ok).assertTrue();
      expect(outcomeRes.data?.applied_files).assertEqual(1);
      expect(outcomeRes.data?.backup_ref).assertEqual('refs/orbit/handoff-backup/1');
      // P1-5: Store execution must NOT directly pollute global toast
      expect(store.toastRevision).assertEqual(toastBefore);

      // Cross-session isolation on modal route binding
      store.openWorktreeHandoffSheet('h-wt-a', 'ws-wt-a', 'ag-wt-a', '/repo-wt-a', 'WT A');
      expect(store.shellStore.modalRoute?.kind).assertEqual('worktree_handoff');
      expect(store.shellStore.modalRoute?.hostId).assertEqual('h-wt-a');
      expect(store.shellStore.modalRoute?.workspaceId).assertEqual('ws-wt-a');

      store.openWorktreeHandoffSheet('h-wt-b', 'ws-wt-b', 'ag-wt-b', '/repo-wt-b', 'WT B');
      expect(store.shellStore.modalRoute?.hostId).assertEqual('h-wt-b');
      expect(store.shellStore.modalRoute?.workspaceId).assertEqual('ws-wt-b');
      store.sidePanelStore.dispose();
    });

    it('projectMenuAuthoritativeWorkspacesAndSingleScriptsDispatch', 0, async () => {
      const bridge = new FakePr5CoreBridge();
      const scriptsMap = '{"ws-a":[{"workspace_id":"ws-a","name":"dev","command":"npm run dev","kind":"service","status":"running","terminal_id":"t1","port":5173,"proxy_url":"http://127.0.0.1:5173","last_error":null},{"workspace_id":"ws-a","name":"test","command":"cargo test","kind":"command","status":"failed","terminal_id":null,"port":null,"proxy_url":null,"last_error":"exit code 101"}]}';
      // P0-2: Same-cwd A & B, worktree C with source_cwd=/repo, archived D with project_root=/repo, unrelated E
      bridge.detailJsonByHost.set(
        'h1',
        makePr5HostDetailJson(
          'h1',
          [
            '{"workspace_id":"ws-a","title":"Alpha","cwd":"/repo","git_branch":"main","is_worktree":false}',
            '{"workspace_id":"ws-b","title":"Beta","cwd":"/repo","git_branch":"main","is_worktree":false}',
            '{"workspace_id":"ws-c","title":"Worktree C","cwd":"/wt/C","source_cwd":"/repo","git_branch":"feat/c","is_worktree":true}',
            '{"workspace_id":"ws-d","title":"Archived D","cwd":"/wt/D","project_root":"/repo","git_branch":"old","is_worktree":true,"archived_at":"2026-11-20T00:00:00Z"}',
            '{"workspace_id":"ws-other","title":"Other","cwd":"/other","git_branch":"main","is_worktree":false}'
          ],
          [],
          scriptsMap
        )
      );

      const store = new OrbitHarmonyStore();
      store.initializeWithTestBridge(bridge);
      await store.fleetStore.reloadHostAsync('h1');

      // P0-2: Authoritative resolution includes A, B, worktree C, and archived D; excludes Other
      const resolved: WorkspaceDto[] = store.resolveProjectWorkspaces('h1', '/repo', ['/repo']);
      expect(resolved.length).assertEqual(4);
      const ids: string[] = [];
      for (let i = 0; i < resolved.length; i++) {
        ids.push(resolved[i].workspace_id);
      }
      expect(ids.indexOf('ws-a') >= 0).assertTrue();
      expect(ids.indexOf('ws-b') >= 0).assertTrue();
      expect(ids.indexOf('ws-c') >= 0).assertTrue();
      expect(ids.indexOf('ws-d') >= 0).assertTrue();
      expect(ids.indexOf('ws-other') >= 0).assertFalse();

      // P0-2: Capturing immutable (hostId, wsId) per menu item survives workspace list reordering (A,B -> B,A)
      const capturedTargetId = resolved[0].workspace_id;
      bridge.detailJsonByHost.set(
        'h1',
        makePr5HostDetailJson(
          'h1',
          [
            '{"workspace_id":"ws-b","title":"Beta","cwd":"/repo","git_branch":"main","is_worktree":false}',
            '{"workspace_id":"ws-a","title":"Alpha","cwd":"/repo","git_branch":"main","is_worktree":false}'
          ],
          [],
          scriptsMap
        )
      );
      await store.fleetStore.reloadHostAsync('h1');
      const toastBefore = store.toastRevision;
      const archOutcome = await store.archiveWorkspaceV2('h1', capturedTargetId, true);
      expect(archOutcome.ok).assertTrue();
      expect(bridge.archiveWorkspaceV2Calls[0]).assertEqual('h1|ws-a|keep');
      // R2-P1-4: Store confirmed workspace RPCs NEVER pollute global toast directly
      expect(store.toastRevision).assertEqual(toastBefore);

      const restoreOutcome = await store.unarchiveWorkspace('h1', 'ws-d');
      expect(restoreOutcome.ok).assertTrue();
      expect(bridge.unarchiveWorkspaceCalls[0]).assertEqual('h1|ws-d');
      expect(store.toastRevision).assertEqual(toastBefore);

      // R2-P1-2 / R2-P1-4: Rename workspace modal route & confirmed RPC + failure propagation
      store.openRenameWorkspaceSheet('h1', 'ws-a', 'Alpha');
      expect(store.shellStore.modalRoute?.kind).assertEqual('rename_workspace');
      expect(store.shellStore.modalRoute?.hostId).assertEqual('h1');
      expect(store.shellStore.modalRoute?.workspaceId).assertEqual('ws-a');
      expect(store.shellStore.modalRoute?.title).assertEqual('Alpha');

      const renameOk = await store.renameWorkspace('h1', 'ws-a', 'Alpha Renamed');
      expect(renameOk.ok).assertTrue();
      expect(bridge.renameWorkspaceCalls[0]).assertEqual('h1|ws-a|Alpha Renamed');

      bridge.archiveWorkspaceShouldFail = 'workspace not found';
      const archFail = await store.archiveWorkspaceV2('h1', 'ws-missing', false);
      expect(archFail.ok).assertFalse();
      expect(archFail.errorDetail).assertEqual('workspace not found');
      expect(store.toastRevision).assertEqual(toastBefore);
      bridge.archiveWorkspaceShouldFail = '';

      // P1-6: Opening Dev Scripts modal does NOT pre-dispatch requestWorkspaceScripts;
      // sheet lifecycle dispatches strictly 1 outbound request per open!
      bridge.requestScriptsCalls = [];
      store.openWorkspaceScriptsSheet('h1', 'ws-a', 'Alpha');
      expect(store.shellStore.modalRoute?.kind).assertEqual('workspace_scripts');
      expect(bridge.requestScriptsCalls.length).assertEqual(0);
      // Simulated WorkspaceScriptsSheet.aboutToAppear()
      expect(store.requestWorkspaceScripts('h1', 'ws-a')).assertTrue();
      expect(bridge.requestScriptsCalls.length).assertEqual(1);
      expect(bridge.requestScriptsCalls[0]).assertEqual('h1|ws-a');

      const scripts: WorkspaceScriptInfoDto[] = store.getWorkspaceScripts('h1', 'ws-a');
      expect(scripts.length).assertEqual(2);
      expect(formatWorkspaceScriptStatusSummary(scripts[0])).assertEqual('http://127.0.0.1:5173');
      expect(formatWorkspaceScriptSummary(scripts[1])).assertEqual('exit code 101');
      expect(store.getWorkspaceScripts('h1', 'ws-other').length).assertEqual(0);

      // P1-6: Start/Stop do NOT trigger immediate reloadHostAsync (updates come via WorkspaceScriptUpdated -> host_changed)
      bridge.detailAsyncCalls = [];
      expect(store.startWorkspaceScript('h1', 'ws-a', 'test')).assertTrue();
      expect(store.stopWorkspaceScript('h1', 'ws-a', 'dev')).assertTrue();
      expect(bridge.startScriptCalls[0]).assertEqual('h1|ws-a|test');
      expect(bridge.stopScriptCalls[0]).assertEqual('h1|ws-a|dev');
      expect(bridge.detailAsyncCalls.length).assertEqual(0);

      expect(
        store.createWorkspaceV2({
          host_id: 'h1',
          source_cwd: '/repo',
          title: 'PR-5 Worktree',
          isolation: 'worktree',
          worktree_mode: 'branch_off',
          worktree_branch: 'feat/pr5',
          base_ref: 'main',
          pr_number: null
        })
      ).assertTrue();
      expect(bridge.createWorkspaceV2Calls.length).assertEqual(1);
      expect(bridge.createWorkspaceV2Calls[0].indexOf('"isolation":"worktree"') >= 0).assertTrue();
      store.sidePanelStore.dispose();
    });

    it('boardCasLegacyDaemonDowngradeBlocksNormalSavePreservesStoreDraftAndAllowsForce', 0, async () => {
      const bridge = new FakePr5CoreBridge();
      // R2-P0-A: Legacy daemon advertises board_cas_supported = false
      bridge.detailJsonByHost.set(
        'h_old',
        makePr5HostDetailJson(
          'h_old',
          ['{"workspace_id":"ws-old","title":"Old","cwd":"/repo","git_branch":"main","is_worktree":false}'],
          [],
          '{}',
          false
        )
      );
      bridge.boardJsonByKey.set(
        'h_old|ws-old',
        '{"workspace_id":"ws-old","content":"existing","revision":0,"updated_by":"peer","updated_at":"2026-11-27T00:00:00Z","board_path":"/repo/BOARD.md"}'
      );
      bridge.boardCasServerRevByKey.set('h_old|ws-old', 0);
      bridge.boardCasServerContentByKey.set('h_old|ws-old', 'existing');

      const store = new OrbitHarmonyStore();
      store.initializeWithTestBridge(bridge);
      await store.fleetStore.reloadHostAsync('h_old');
      expect(store.isHostBoardCasSupported('h_old')).assertFalse();

      // R2-P1-1: Persist draft in Store so it survives component unmount/remount
      const key = encodeSidePanelKeyTuple(['board_ws', 'h_old', 'ws-old']);
      store.setWorkspaceBoardDraft(key, 'my unsaved edits', 0);
      const restored = store.getWorkspaceBoardDraft(key);
      expect(restored !== null).assertTrue();
      expect(restored?.content).assertEqual('my unsaved edits');
      expect(restored?.baseRevision).assertEqual(0);

      // Normal CAS save (rev 0) on legacy daemon is rejected locally with ZERO wire writes
      const blockedRev0 = await store.saveScopedWorkspaceBoardWithCas('h_old', 'ws-old', 'my unsaved edits', 0, false, '/repo', null);
      expect(blockedRev0.ok).assertFalse();
      expect(blockedRev0.errorCode).assertEqual('cas_unsupported');
      expect(bridge.updateBoardCasCalls.length).assertEqual(0);
      // Draft remains intact in Store
      expect(store.getWorkspaceBoardDraft(key)?.content).assertEqual('my unsaved edits');

      // Explicit force overwrite (`force = true`) is permitted and sends expected_revision: null
      const forceRes = await store.saveScopedWorkspaceBoardWithCas('h_old', 'ws-old', 'my unsaved edits', 0, true, '/repo', null);
      expect(forceRes.ok).assertTrue();
      expect(bridge.updateBoardCasCalls.length).assertEqual(1);
      expect(bridge.updateBoardCasCalls[0].indexOf('"expected_revision":null') >= 0).assertTrue();
      store.clearWorkspaceBoardDraft(key);
      expect(store.getWorkspaceBoardDraft(key)).assertNull();
      store.sidePanelStore.dispose();
    });

    it('chunkedFileMergeRejectsBinaryTailNoProgressAndEtagConflict', 0, () => {
      const chunk0: WorkspaceFileViewDto = {
        host_id: 'h1',
        cwd: '/repo',
        path: 'mixed.txt',
        resolved_path: '/repo/mixed.txt',
        language: 'text',
        content_kind: 'text',
        mime_type: 'text/plain',
        content: 'AAAA',
        size_bytes: 2097165,
        truncated: true,
        etag: 'sha256-full',
        mtime_unix_ms: 1700000000000,
        writable: true,
        line_ending: 'lf',
        offset_bytes: 0,
        next_offset_bytes: 2097152,
        etag_mismatch: false,
        highlighted_lines: [],
        markdown_blocks: []
      };

      // 1. R2-P0-C: Binary tail chunk with truncated=false and 0 B progress MUST lock read-only!
      const binaryTailChunk: WorkspaceFileViewDto = {
        host_id: 'h1',
        cwd: '/repo',
        path: 'mixed.txt',
        resolved_path: '/repo/mixed.txt',
        language: 'text',
        content_kind: 'binary',
        mime_type: null,
        content: '',
        size_bytes: 2097165,
        truncated: false,
        etag: 'sha256-full',
        mtime_unix_ms: 1700000000000,
        writable: true,
        line_ending: null,
        offset_bytes: 2097152,
        next_offset_bytes: 2097152,
        etag_mismatch: false,
        highlighted_lines: [],
        markdown_blocks: []
      };
      const binRes = mergeWorkspaceFileChunk(chunk0, 2097152, binaryTailChunk, 'h1', '/repo', 'mixed.txt', 'sha256-full');
      expect(binRes.ok).assertFalse();
      expect(binRes.binaryTail).assertTrue();
      expect(binRes.readOnlyLocked).assertTrue();
      expect(binRes.mergedView.truncated).assertTrue();
      expect(binRes.mergedView.writable).assertFalse();
      expect(isWorkspaceFileEditable(binRes.mergedView)).assertFalse();

      // 2. Valid UTF-8 text tail completing all 2,097,165 bytes unlocks editing cleanly
      const validTailChunk: WorkspaceFileViewDto = {
        host_id: 'h1',
        cwd: '/repo',
        path: 'mixed.txt',
        resolved_path: '/repo/mixed.txt',
        language: 'text',
        content_kind: 'text',
        mime_type: 'text/plain',
        content: 'TAIL_UTF8_🚀',
        size_bytes: 2097165,
        truncated: false,
        etag: 'sha256-full',
        mtime_unix_ms: 1700000000000,
        writable: true,
        line_ending: 'lf',
        offset_bytes: 2097152,
        next_offset_bytes: 2097165,
        etag_mismatch: false,
        highlighted_lines: [],
        markdown_blocks: []
      };
      const okRes = mergeWorkspaceFileChunk(chunk0, 2097152, validTailChunk, 'h1', '/repo', 'mixed.txt', 'sha256-full');
      expect(okRes.ok).assertTrue();
      expect(okRes.mergedView.truncated).assertFalse();
      expect(okRes.mergedView.content).assertEqual('AAAATAIL_UTF8_🚀');
      expect(isWorkspaceFileEditable(okRes.mergedView)).assertTrue();

      // 3. ETag mismatch locks read-only
      const conflictChunk: WorkspaceFileViewDto = {
        host_id: 'h1',
        cwd: '/repo',
        path: 'mixed.txt',
        resolved_path: '/repo/mixed.txt',
        language: 'text',
        content_kind: 'text',
        mime_type: 'text/plain',
        content: '',
        size_bytes: 2097200,
        truncated: true,
        etag: 'sha256-changed',
        mtime_unix_ms: 1700000009999,
        writable: true,
        line_ending: null,
        offset_bytes: 2097152,
        next_offset_bytes: 2097152,
        etag_mismatch: true,
        highlighted_lines: [],
        markdown_blocks: []
      };
      const etagRes = mergeWorkspaceFileChunk(chunk0, 2097152, conflictChunk, 'h1', '/repo', 'mixed.txt', 'sha256-full');
      expect(etagRes.ok).assertFalse();
      expect(etagRes.etagMismatch).assertTrue();
      expect(isWorkspaceFileEditable(etagRes.mergedView)).assertFalse();
    });

    it('authoritativeProjectOwnershipMatchesCoreLongestPrefixAndProjectRoot', 0, async () => {
      // R2-P1-2 / R3-P1-2: Lock ALL 9 ownership cases matching `sidebar.rs` verbatim:
      // `/repo`, `/repo/nested`, `/other`, `RootWins`, `UnmatchedRoot`, `LegacyWorktree`,
      // duplicate dirs A/B, source alias C, and archived D — both via fallback matcher AND via
      // authoritative Core `SidebarProjectGroupDto.workspace_ids` projection!
      const groups: OrbitProjectDirectoryGroupInput[] = [
        { id: 'g_repo', primaryCwd: '/repo', directories: ['/repo', '/common'] },
        { id: 'g_nested', primaryCwd: '/repo/nested', directories: ['/repo/nested'] },
        { id: 'g_other', primaryCwd: '/other', directories: ['/other', '/common'] }
      ];
      const groupsReversed: OrbitProjectDirectoryGroupInput[] = [
        { id: 'g_other', primaryCwd: '/other', directories: ['/other', '/common'] },
        { id: 'g_nested', primaryCwd: '/repo/nested', directories: ['/repo/nested'] },
        { id: 'g_repo', primaryCwd: '/repo', directories: ['/repo', '/common'] }
      ];
      const workspaces: WorkspaceDto[] = [
        { workspace_id: 'ws_dup_a', title: 'Dup A', cwd: '/repo', git_branch: 'main', is_worktree: false },
        { workspace_id: 'ws_dup_b', title: 'Dup B', cwd: '/repo', git_branch: 'main', is_worktree: false },
        { workspace_id: 'ws_alias_c', title: 'Alias C', cwd: '/wt/c', source_cwd: '/repo', git_branch: 'feat/c', is_worktree: true },
        { workspace_id: 'ws_arch_d', title: 'Archived D', cwd: '/wt/d', project_root: '/repo', git_branch: 'old', is_worktree: true, archived_at: '2026-11-20T00:00:00Z' },
        { workspace_id: 'ws_legacy_wt', title: 'LegacyWorktree', cwd: '/home/me/.orbit/worktrees/repo-branch', git_branch: 'branch', is_worktree: true },
        { workspace_id: 'ws_nested', title: 'Nested', cwd: '/repo/nested', git_branch: 'main', is_worktree: false },
        { workspace_id: 'ws_nested_wt', title: 'Nested WT', cwd: '/wt/nested-wt', source_cwd: '/repo/nested/sub', git_branch: 'feat/sub', is_worktree: true },
        { workspace_id: 'ws_unmatched_root', title: 'UnmatchedRoot', cwd: '/repo/nested', project_root: '/outside/declared', git_branch: 'main', is_worktree: false },
        { workspace_id: 'ws_root_wins', title: 'RootWins', cwd: '/repo/wt-x', source_cwd: '/repo', project_root: '/other', git_branch: 'feat/other', is_worktree: true },
        { workspace_id: 'ws_shared_child', title: 'SharedChild', cwd: '/common/child', git_branch: 'main', is_worktree: false }
      ];

      // R4-Nit: Equal-length `/common` alias tie-break MUST resolve SharedChild to `/other`
      // identically regardless of UI display sort order (`[repo, other]` vs `[other, repo]`).
      const repoOwned = resolveAuthoritativeProjectWorkspaces(workspaces, groups, 'g_repo');
      const nestedOwned = resolveAuthoritativeProjectWorkspaces(workspaces, groups, 'g_nested');
      const otherOwned = resolveAuthoritativeProjectWorkspaces(workspaces, groups, 'g_other');
      const repoOwnedRev = resolveAuthoritativeProjectWorkspaces(workspaces, groupsReversed, 'g_repo');
      const otherOwnedRev = resolveAuthoritativeProjectWorkspaces(workspaces, groupsReversed, 'g_other');

      expect(repoOwned.map((w: WorkspaceDto): string => w.workspace_id).join(',')).assertEqual(
        'ws_dup_a,ws_dup_b,ws_alias_c,ws_arch_d,ws_legacy_wt'
      );
      expect(repoOwnedRev.map((w: WorkspaceDto): string => w.workspace_id).join(',')).assertEqual(
        'ws_dup_a,ws_dup_b,ws_alias_c,ws_arch_d,ws_legacy_wt'
      );
      expect(nestedOwned.map((w: WorkspaceDto): string => w.workspace_id).join(',')).assertEqual(
        'ws_nested,ws_nested_wt,ws_unmatched_root'
      );
      expect(otherOwned.map((w: WorkspaceDto): string => w.workspace_id).join(',')).assertEqual(
        'ws_root_wins,ws_shared_child'
      );
      expect(otherOwnedRev.map((w: WorkspaceDto): string => w.workspace_id).join(',')).assertEqual(
        'ws_root_wins,ws_shared_child'
      );

      // R5-Nit-2: Unicode scalar comparison (`compareUnicodeCodePoints`) matches Rust UTF-8
      // `str::cmp` (`U+FF21` '/ＡＰＰ' < `U+1F680` '/🚀工具'), avoiding UTF-16 surrogate inversion!
      expect(compareUnicodeCodePoints('/ＡＰＰ', '/🚀工具') < 0).assertTrue();
      expect(compareUnicodeCodePoints('/🚀工具', '/ＡＰＰ') > 0).assertTrue();
      const unicodeGroupsA: OrbitProjectDirectoryGroupInput[] = [
        { id: 'g_rocket', primaryCwd: '/🚀工具', directories: ['/🚀工具', '/common'] },
        { id: 'g_fullwidth', primaryCwd: '/ＡＰＰ', directories: ['/ＡＰＰ', '/common'] }
      ];
      const unicodeGroupsB: OrbitProjectDirectoryGroupInput[] = [
        { id: 'g_fullwidth', primaryCwd: '/ＡＰＰ', directories: ['/ＡＰＰ', '/common'] },
        { id: 'g_rocket', primaryCwd: '/🚀工具', directories: ['/🚀工具', '/common'] }
      ];
      const sharedChildOnly: WorkspaceDto[] = [
        { workspace_id: 'ws_shared_child', title: 'SharedChild', cwd: '/common/child', git_branch: 'main', is_worktree: false }
      ];
      expect(resolveAuthoritativeProjectWorkspaces(sharedChildOnly, unicodeGroupsA, 'g_fullwidth').length).assertEqual(1);
      expect(resolveAuthoritativeProjectWorkspaces(sharedChildOnly, unicodeGroupsA, 'g_rocket').length).assertEqual(0);
      expect(resolveAuthoritativeProjectWorkspaces(sharedChildOnly, unicodeGroupsB, 'g_fullwidth').length).assertEqual(1);
      expect(resolveAuthoritativeProjectWorkspaces(sharedChildOnly, unicodeGroupsB, 'g_rocket').length).assertEqual(0);

      // Verify Store consumes authoritative Core `SidebarProjectGroupDto.workspace_ids` directly!
      const bridge = new FakePr5CoreBridge();
      const sidebarPayload: FleetSidebarDto = {
        revision: 2,
        projects: [
          {
            id: 'proj_repo',
            host_id: 'h1',
            name: 'repo',
            primary_cwd: '/repo',
            directories: ['/repo'],
            git_branch: 'main',
            pinned_project_id: 'proj_repo',
            is_home_workspace: false,
            sort_weight_ms: 100,
            conversations: [],
            workspace_ids: ['ws_dup_a', 'ws_dup_b', 'ws_alias_c', 'ws_arch_d', 'ws_legacy_wt']
          },
          {
            id: 'proj_nested',
            host_id: 'h1',
            name: 'nested',
            primary_cwd: '/repo/nested',
            directories: ['/repo/nested'],
            git_branch: 'main',
            pinned_project_id: 'proj_nested',
            is_home_workspace: false,
            sort_weight_ms: 90,
            conversations: [],
            workspace_ids: ['ws_nested', 'ws_unmatched_root']
          }
        ],
        launchpad_projects: [],
        needs_attention: [],
        pinned_conversations: []
      };
      bridge.sidebarJson = JSON.stringify(sidebarPayload);
      const wsJsonList: string[] = [];
      for (let i = 0; i < workspaces.length; i++) {
        wsJsonList.push(JSON.stringify(workspaces[i]));
      }
      bridge.detailJsonByHost.set('h1', makePr5HostDetailJson('h1', wsJsonList, []));
      const store = new OrbitHarmonyStore();
      store.initializeWithTestBridge(bridge);
      await store.fleetStore.reloadHostAsync('h1');
      store.fleetStore.applySidebarView(JSON.parse(bridge.sidebarJson) as FleetSidebarDto);

      const viaCoreRepo = store.resolveProjectWorkspaces('h1', '/repo', ['/repo'], 'proj_repo');
      const viaCoreNested = store.resolveProjectWorkspaces('h1', '/repo/nested', ['/repo/nested'], 'proj_nested');
      expect(viaCoreRepo.map((w: WorkspaceDto): string => w.workspace_id).join(',')).assertEqual(
        'ws_dup_a,ws_dup_b,ws_alias_c,ws_arch_d,ws_legacy_wt'
      );
      expect(viaCoreNested.map((w: WorkspaceDto): string => w.workspace_id).join(',')).assertEqual(
        'ws_nested,ws_unmatched_root'
      );
      store.sidePanelStore.dispose();
    });

    it('syncHighlightPreClips16MiBSingleLineSafelyWithoutSplittingSurrogates', 0, () => {
      const countLogicalLines = (text: string): number => {
        if (text.length === 0) {
          return 0;
        }
        const split = text.split('\n');
        return text.endsWith('\n') ? split.length - 1 : split.length;
      };

      // R2-P1-3 / R3-P1-3 / R4-P1-2 / R5-P1: 16 MiB single-line string pre-clips inside 512 KiB
      // budget BEFORE JSON.stringify, records `plainLineNumbers: [1]`, and never splits surrogates.
      const oneLine16MiB = 'const x=1;'.repeat(1677722);
      const t0 = Date.now();
      const clipped = clipCodeForSyncHighlightPreview(oneLine16MiB, 250);
      const elapsedMs = Date.now() - t0;
      expect(clipped.preTruncated).assertTrue();
      expect(clipped.totalLines).assertEqual(1);
      expect(clipped.totalLinesExact).assertFalse();
      expect(clipped.clippedCode.length <= 4096).assertTrue();
      expect(clipped.plainLineNumbers.length).assertEqual(1);
      expect(clipped.plainLineNumbers[0]).assertEqual(1);
      expect(elapsedMs < 20).assertTrue();

      // Surrogate pair boundary check (> 512 KiB single-line): 'a' * 4095 + '🚀' + 'b' * 600000
      const withEmojiAtBoundary = `${'a'.repeat(4095)}🚀${'b'.repeat(600000)}`;
      const emojiClip = clipCodeForSyncHighlightPreview(withEmojiAtBoundary, 250);
      expect(emojiClip.preTruncated).assertTrue();
      expect(emojiClip.totalLinesExact).assertFalse();
      expect(emojiClip.clippedCode.length).assertEqual(4095);
      expect(emojiClip.plainLineNumbers.length).assertEqual(1);
      expect(emojiClip.plainLineNumbers[0]).assertEqual(1);
      const lastUnit = emojiClip.clippedCode.charCodeAt(emojiClip.clippedCode.length - 1);
      expect(lastUnit >= 0xD800 && lastUnit <= 0xDBFF).assertFalse();

      // R4-P1-2 & R5-P1 Case A: 16,002 lines with line 1 = 7,000 / 65,535 / 65,536 chars
      // -> MUST extract ALL 250 lines (line 1 plain, lines 2..250 highlighted), exact totalLines = 16002!
      const line1Lengths: number[] = [7000, 65535, 65536];
      for (let idx = 0; idx < line1Lengths.length; idx++) {
        const targetLen = line1Lengths[idx];
        const mixed = `// ${'c'.repeat(targetLen - 3)}\n${'const val = 42;\n'.repeat(16001)}`;
        const mixedClip = clipCodeForSyncHighlightPreview(mixed, 250);
        expect(mixedClip.preTruncated).assertTrue();
        expect(mixedClip.totalLines).assertEqual(16002);
        expect(mixedClip.totalLinesExact).assertTrue();
        expect(mixedClip.plainLineNumbers.length).assertEqual(1);
        expect(mixedClip.plainLineNumbers[0]).assertEqual(1);
        expect(countLogicalLines(mixedClip.clippedCode)).assertEqual(250);
        const mixedLines = mixedClip.clippedCode.split('\n');
        expect(mixedLines[0].length).assertEqual(4096);
        expect(mixedLines[1]).assertEqual('const val = 42;');
      }

      // R5-P1 Case B: 16,003 lines with line 2 = 65,536 chars -> extracts ALL 250 lines
      // (line 1 & line 3..250 normal JS, ONLY line 2 plain), exact totalLines = 16003!
      const midLine65536 = `const first = 1;\n// ${'c'.repeat(65533)}\n${'const val = 42;\n'.repeat(16001)}`;
      const midClip = clipCodeForSyncHighlightPreview(midLine65536, 250);
      expect(midClip.preTruncated).assertTrue();
      expect(midClip.totalLines).assertEqual(16003);
      expect(midClip.totalLinesExact).assertTrue();
      expect(midClip.plainLineNumbers.length).assertEqual(1);
      expect(midClip.plainLineNumbers[0]).assertEqual(2);
      expect(countLogicalLines(midClip.clippedCode)).assertEqual(250);
      const midLines = midClip.clippedCode.split('\n');
      expect(midLines[0]).assertEqual('const first = 1;');
      expect(midLines[1].length).assertEqual(4096);
      expect(midLines[2]).assertEqual('const val = 42;');

      // R5-Nit-1: Large 16,250-line file where line 250 is an empty line `""` MUST preserve
      // line 250's `\n` terminator so `render.rs` emits all 250 lines (never dropping to 249)!
      const emptyLine250File = `${'const a = 12345;\n'.repeat(249)}\n${'const b = 67890;\n'.repeat(16000)}`;
      const empty250Clip = clipCodeForSyncHighlightPreview(emptyLine250File, 250);
      expect(empty250Clip.preTruncated).assertTrue();
      expect(empty250Clip.totalLines).assertEqual(16250);
      expect(empty250Clip.totalLinesExact).assertTrue();
      expect(empty250Clip.clippedCode.endsWith('\n\n')).assertTrue();
      expect(countLogicalLines(empty250Clip.clippedCode)).assertEqual(250);

      // R6-P1: 15-case scanWindow 512 Ki boundary test (5 consecutive offsets × ASCII / CJK / emoji 🚀)
      // in a 3-line file where line 1 is ~524,200 chars and line 2 straddles the 512 Ki boundary:
      // MUST NEVER split a UTF-16 surrogate pair, keep both visible lines, and mark exact=false!
      const isWellFormedUtf16 = (s: string): boolean => {
        for (let i = 0; i < s.length; i++) {
          const u = s.charCodeAt(i);
          if (u >= 0xD800 && u <= 0xDBFF) {
            if (i + 1 >= s.length) {
              return false;
            }
            const next = s.charCodeAt(i + 1);
            if (next < 0xDC00 || next > 0xDFFF) {
              return false;
            }
            i += 1;
          } else if (u >= 0xDC00 && u <= 0xDFFF) {
            return false;
          }
        }
        return true;
      };
      const scanCap = 512 * 1024;
      const line1Pad = `// ${'x'.repeat(scanCap - 35)}\n`;
      const boundaryChars: string[] = ['X', '汉', '🚀'];
      for (let cIdx = 0; cIdx < boundaryChars.length; cIdx++) {
        const ch = boundaryChars[cIdx];
        for (let offset = -2; offset <= 2; offset++) {
          const targetStart = scanCap + offset;
          const line2PrefixLen = targetStart - line1Pad.length;
          const line2 = `${'a'.repeat(line2PrefixLen)}${ch}tail_end\n`;
          const doc = `${line1Pad}${line2}const line3 = 3;\n`;
          expect(isWellFormedUtf16(doc)).assertTrue();
          const winClip = clipCodeForSyncHighlightPreview(doc, 250);
          expect(isWellFormedUtf16(winClip.clippedCode)).assertTrue();
          expect(winClip.preTruncated).assertTrue();
          expect(winClip.totalLinesExact).assertFalse();
          expect(countLogicalLines(winClip.clippedCode)).assertEqual(2);
          expect(winClip.plainLineNumbers.length).assertEqual(1);
          expect(winClip.plainLineNumbers[0]).assertEqual(1);
        }
      }

      // R7-Nit: Emoji `🚀` straddling the internal newline-probe boundary (`pos + takeLimit + 2 = 4098`)
      // in a 16,002-line JS file must remain 100% well-formed across every slice, returning 250 lines
      // and exact `totalLines === 16002`.
      const probeBoundaryEmojiDoc = `// ${'c'.repeat(4094)}🚀tail_after_emoji\n${'const val = 42;\n'.repeat(16001)}`;
      expect(isWellFormedUtf16(probeBoundaryEmojiDoc)).assertTrue();
      const probeBoundaryClip = clipCodeForSyncHighlightPreview(probeBoundaryEmojiDoc, 250);
      expect(isWellFormedUtf16(probeBoundaryClip.clippedCode)).assertTrue();
      expect(probeBoundaryClip.preTruncated).assertTrue();
      expect(probeBoundaryClip.totalLines).assertEqual(16002);
      expect(probeBoundaryClip.totalLinesExact).assertTrue();
      expect(probeBoundaryClip.plainLineNumbers.length).assertEqual(1);
      expect(probeBoundaryClip.plainLineNumbers[0]).assertEqual(1);
      expect(countLogicalLines(probeBoundaryClip.clippedCode)).assertEqual(250);

      // R4-P1-2 Case C: 2.75 MB / 250,000 regular JS lines -> extracts ALL 250 lines with 0 plain
      // overrides and explicitly marks `totalLinesExact === false`.
      const huge250k = 'const n=1;\n'.repeat(250000);
      const hugeClip = clipCodeForSyncHighlightPreview(huge250k, 250);
      expect(hugeClip.preTruncated).assertTrue();
      expect(hugeClip.totalLinesExact).assertFalse();
      expect(hugeClip.plainLineNumbers.length).assertEqual(0);
      expect(countLogicalLines(hugeClip.clippedCode)).assertEqual(250);
      expect(hugeClip.totalLines >= 250).assertTrue();
      expect(hugeClip.totalLines < 250000).assertTrue();

      const mixed16002 = `// ${'c'.repeat(65533)}\n${'const val = 42;\n'.repeat(16001)}`;

      // Store wrapper passes clipped payload (< 8 KiB) + `plain_line_numbers: [1]` + propagates `total_lines_exact`
      const bridge = new FakePr5CoreBridge();
      bridge.highlightResponseJson = '{"language":"js","lines":[{"line_number":1,"tokens":[{"text":"const x=1;","fg_hex":"#E2E8F0","light_fg_hex":"#24292F","bold":false,"italic":false}]}],"total_lines":1,"total_lines_exact":true,"truncated":true}';
      const store = new OrbitHarmonyStore();
      store.initializeWithTestBridge(bridge);
      const res = store.highlightScopedWorkspaceCode('js', oneLine16MiB, 250);
      expect(res !== null).assertTrue();
      expect(res?.truncated).assertTrue();
      expect(res?.total_lines).assertEqual(1);
      expect(res?.total_lines_exact).assertFalse();
      expect(bridge.highlightCalls.length).assertEqual(1);
      expect(bridge.highlightCalls[0].length < 8192).assertTrue();
      expect(bridge.highlightCalls[0].indexOf('"plain_line_numbers":[1]') >= 0).assertTrue();

      const res250k = store.highlightScopedWorkspaceCode('js', huge250k, 250);
      expect(res250k !== null).assertTrue();
      expect(res250k?.truncated).assertTrue();
      expect(res250k?.total_lines_exact).assertFalse();

      const resMixed = store.highlightScopedWorkspaceCode('js', mixed16002, 250);
      expect(resMixed !== null).assertTrue();
      expect(resMixed?.truncated).assertTrue();
      expect(resMixed?.total_lines).assertEqual(16002);
      expect(resMixed?.total_lines_exact).assertTrue();

      // Verify Store fallback on exact 512 Ki emoji-straddling 3-line doc produces well-formed
      // JSON (no lone surrogate) with 2 visible lines and `total_lines_exact === false`.
      const emojiWindowDoc = `${line1Pad}${'a'.repeat((scanCap - 1) - line1Pad.length)}🚀tail\nconst line3 = 3;\n`;
      bridge.highlightResponseJson = '{"language":"js","lines":[{"line_number":1,"tokens":[{"text":"// xxx","fg_hex":"#E2E8F0","light_fg_hex":"#24292F","bold":false,"italic":false}]},{"line_number":2,"tokens":[{"text":"aaa","fg_hex":"#E2E8F0","light_fg_hex":"#24292F","bold":false,"italic":false}]}],"total_lines":2,"total_lines_exact":true,"truncated":true}';
      const resEmojiWin = store.highlightScopedWorkspaceCode('js', emojiWindowDoc, 250);
      expect(resEmojiWin !== null).assertTrue();
      expect(resEmojiWin?.lines.length).assertEqual(2);
      expect(resEmojiWin?.total_lines_exact).assertFalse();
      const lastCallJson = bridge.highlightCalls[bridge.highlightCalls.length - 1];
      expect(isWellFormedUtf16(lastCallJson)).assertTrue();
      store.sidePanelStore.dispose();
    });

    it('lateWorkspaceMutationsSilencedAcrossHostSwitchRoundTripAndUnmount', 0, async () => {
      // R3-P1-4: Verify monotonic `hostSelectionEpoch` invalidates late Archive/Restore/Rename
      // presentation feedback across `h1 -> h2`, `h1 -> h2 -> h1`, and sheet unmount/reopen!
      const bridge = new FakePr5CoreBridge();
      bridge.fleetJson = '{"revision":1,"hosts":[{"host_id":"h1","display_name":"H1","hostname":"h1","custom_name":null,"ssh_alias":null,"route_mode":"auto","remote_daemon_port":6868,"custom_direct_endpoint":null,"via_ssh_tunnel":false,"last_ping_ms":5,"daemon_version":"1.0","os_arch":"linux","daemon_pubkey_b64":"","direct_endpoints":[],"relay_url":null,"connection_state":"online_direct","last_error":null,"available_providers":["claude"],"workspace_count":1,"agent_count":0,"pending_approval_count":0,"board_cas_supported":true,"is_local":false},{"host_id":"h2","display_name":"H2","hostname":"h2","custom_name":null,"ssh_alias":null,"route_mode":"auto","remote_daemon_port":6868,"custom_direct_endpoint":null,"via_ssh_tunnel":false,"last_ping_ms":5,"daemon_version":"1.0","os_arch":"linux","daemon_pubkey_b64":"","direct_endpoints":[],"relay_url":null,"connection_state":"online_direct","last_error":null,"available_providers":["claude"],"workspace_count":1,"agent_count":0,"pending_approval_count":0,"board_cas_supported":true,"is_local":false}],"projects":[],"total_pending_approvals":0}';
      bridge.detailJsonByHost.set('h1', makePr5HostDetailJson('h1', ['{"workspace_id":"ws-1","title":"One","cwd":"/repo","git_branch":"main","is_worktree":false}'], []));
      bridge.detailJsonByHost.set('h2', makePr5HostDetailJson('h2', ['{"workspace_id":"ws-2","title":"Two","cwd":"/repo2","git_branch":"main","is_worktree":false}'], []));

      const store = new OrbitHarmonyStore();
      store.initializeWithTestBridge(bridge);
      store.selectHost('h1');
      const epochH1First = store.hostSelectionEpoch;
      expect(store.selectedHostId).assertEqual('h1');
      expect(epochH1First > 0).assertTrue();

      // 1. Switch h1 -> h2: epoch advances; origin guard for h1/epochH1First becomes false!
      store.selectHost('h2');
      const epochH2 = store.hostSelectionEpoch;
      expect(store.selectedHostId).assertEqual('h2');
      expect(epochH2 > epochH1First).assertTrue();
      const canShowAfterH1ToH2 = store.selectedHostId === 'h1' && store.hostSelectionEpoch === epochH1First;
      expect(canShowAfterH1ToH2).assertFalse();

      // 2. Round-trip h1 -> h2 -> h1: selectedHostId returns to 'h1' BUT epoch strictly increases!
      store.selectHost('h1');
      const epochH1Second = store.hostSelectionEpoch;
      expect(store.selectedHostId).assertEqual('h1');
      expect(epochH1Second > epochH2).assertTrue();
      const canShowAfterRoundTrip = store.selectedHostId === 'h1' && store.hostSelectionEpoch === epochH1First;
      expect(canShowAfterRoundTrip).assertFalse();

      // 3. Stale RPCs still refresh h1's cache without touching global toastRevision
      const toastBefore = store.toastRevision;
      bridge.detailAsyncCalls = [];
      const lateArchive = await store.archiveWorkspaceV2('h1', 'ws-1', false);
      expect(lateArchive.ok).assertTrue();
      expect(bridge.detailAsyncCalls.indexOf('h1') >= 0).assertTrue();
      expect(store.toastRevision).assertEqual(toastBefore);

      bridge.unarchiveWorkspaceShouldFail = 'not archived';
      const lateRestoreFail = await store.unarchiveWorkspace('h1', 'ws-1');
      expect(lateRestoreFail.ok).assertFalse();
      expect(store.toastRevision).assertEqual(toastBefore);
      bridge.unarchiveWorkspaceShouldFail = '';

      const lateRename = await store.renameWorkspace('h1', 'ws-1', 'New Name');
      expect(lateRename.ok).assertTrue();
      expect(store.toastRevision).assertEqual(toastBefore);
      store.sidePanelStore.dispose();
    });
  });
}
