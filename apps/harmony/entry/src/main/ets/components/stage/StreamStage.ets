import { SymbolGlyphModifier } from '@kit.ArkUI';
import {
  AgentCardDto,
  HostCardDto,
  LaunchpadProjectDto,
  SessionSubagentEntryDto,
  basenameOfPath,
  cwdPathsEquivalent,
  formatCompactTopBarConnectionLabel
} from '../../model/OrbitTypes';
import { OrbitStageTurnModel } from '../../model/viewmodels/OrbitStageModels';
import { OrbitFleetStore } from '../../model/stores/OrbitFleetStore';
import { OrbitStageStore } from '../../model/stores/OrbitStageStore';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitButtonSubhead,
  orbitInputMono,
  orbitSurface,
  orbitTextBody,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead,
  orbitTextTitle
} from '../../theme/OrbitTokens';
import {
  dismissHarmonyKeyboard,
  resolveHostStatusColor,
  resolveSelectedHost
} from '../common/OrbitCommonControls';
import {
  OrbitAgentListItem,
  mapAgentCardToListStatus,
  mapCrewSubagentStatus,
  resolveAgentEffortDisplay,
  resolveAgentFallbackLabel,
  resolveCompactAgentModelLabel,
  resolveCrewSubagentDepth,
  resolveCrewSubagentHostId
} from '../common/OrbitAgentListItem';
import {
  OrbitAvatarSeedKeyed,
  OrbitSubagentAvatarAsset,
  assignSubagentCharacters
} from '../common/OrbitSubagentAvatar';
import { StageTurnView } from './StageTurnView';
import { OrbitPullRefreshController } from '../settings/OrbitSettingsNavModel';

interface StageCrewAvatarSeed extends OrbitAvatarSeedKeyed {
  readonly key: string;
  readonly seed: string;
}

@ComponentV2
export struct StreamStage {
  @Local firstVisibleIndex: number = 0;
  @Local lastVisibleIndex: number = 0;
  @Local atBottomEdge: boolean = true;
  @Local manualPairUri: string = '';
  @Local isRefreshing: boolean = false;

  @Local private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();
  @Local private stageStore: OrbitStageStore = OrbitHarmonyStore.getInstance().stageStore;
  @Local private fleetStore: OrbitFleetStore = OrbitHarmonyStore.getInstance().fleetStore;
  private listScroller: Scroller = new Scroller();
  private readonly pullRefreshCtrl: OrbitPullRefreshController = new OrbitPullRefreshController();
  private isAutoAnchoringResize: boolean = false;
  private refreshTimerId: number = -1;
  private resizeTimerId: number = -1;

  aboutToDisappear(): void {
    this.pullRefreshCtrl.unmount();
    this.isRefreshing = false;
    if (this.refreshTimerId !== -1) {
      clearTimeout(this.refreshTimerId);
      this.refreshTimerId = -1;
    }
    if (this.resizeTimerId !== -1) {
      clearTimeout(this.resizeTimerId);
      this.resizeTimerId = -1;
    }
  }

  @Computed
  get activeHostCard(): HostCardDto | null {
    const resolved = resolveSelectedHost(this.store);
    if (resolved !== null) {
      return resolved;
    }
    return this.store.hosts.length > 0 ? this.store.hosts[0] : null;
  }

  @Computed
  get launchpadProjects(): LaunchpadProjectDto[] {
    return this.store.launchpadProjects();
  }

  @Computed
  get activeProjectLabel(): string {
    const projects = this.launchpadProjects;
    for (let i = 0; i < projects.length; i++) {
      if (cwdPathsEquivalent(projects[i].path, this.store.activeCwd)) {
        return projects[i].name;
      }
    }
    if (this.store.activeCwd.trim().length > 0 && this.store.activeCwd !== '~') {
      return basenameOfPath(this.store.activeCwd);
    }
    const host = this.activeHostCard;
    if (host !== null && host.display_name.trim().length > 0) {
      return host.display_name.trim();
    }
    return 'Orbit';
  }

  private lastListItemIndex(): number {
    const headerOffset = this.store.stageHasOlderHistory ? 1 : 0;
    return Math.max(0, this.store.stageTurns.length + headerOffset - 1);
  }

  private showJumpToLatest(): boolean {
    const totalItems = this.store.stageTurns.length;
    if (totalItems === 0) {
      return false;
    }
    if (this.lastVisibleIndex < this.lastListItemIndex()) {
      return true;
    }
    return !this.atBottomEdge;
  }

  private handlePullRefresh(): void {
    const scope = `${this.store.selectedHostId}::${this.store.selectedAgentId}`;
    void this.pullRefreshCtrl.run(
      scope,
      (): Promise<void> => this.store.pullRefreshStageAsync(),
      (refreshing: boolean): void => {
        this.isRefreshing = refreshing;
      }
    );
  }

  @Monitor('stageStore.activeRevision')
  onStageUpdated(_mon: IMonitor): void {
    const totalItems = this.store.stageTurns.length;
    if (this.atBottomEdge && totalItems > 0) {
      this.lastVisibleIndex = totalItems - 1;
      this.listScroller.scrollEdge(Edge.Bottom);
    }
  }

  @Monitor('store.selectedAgentId', 'fleetStore.selectedHostId')
  onSelectedAgentChanged(_mon: IMonitor): void {
    const scope = `${this.fleetStore.selectedHostId}::${this.store.selectedAgentId}`;
    if (this.pullRefreshCtrl.onScopeChanged(scope)) {
      this.isRefreshing = false;
    }
    this.firstVisibleIndex = 0;
    this.lastVisibleIndex = Math.max(0, this.store.stageTurns.length - 1);
    this.atBottomEdge = true;
  }

  @Monitor('store.decisionFocusRevision')
  onDecisionFocusRequested(_mon: IMonitor): void {
    this.atBottomEdge = true;
    const totalItems = this.store.stageTurns.length;
    if (totalItems > 0) {
      this.lastVisibleIndex = totalItems - 1;
      this.listScroller.scrollEdge(Edge.Bottom);
    }
  }

  private isHostOnline(host: HostCardDto): boolean {
    return host.connection_state === 'online_direct' || host.connection_state === 'online_relay';
  }

  private hostSubtitle(host: HostCardDto): string {
    const route = formatCompactTopBarConnectionLabel(host.connection_state, host.via_ssh_tunnel);
    if (host.last_ping_ms !== null && host.last_ping_ms !== undefined) {
      return `${route} · ${host.last_ping_ms}ms`;
    }
    return route;
  }

  private isDraftSession(): boolean {
    return this.store.isNewSessionDraft || this.store.selectedAgentId.length === 0;
  }

  @Computed
  get workspacePeers(): AgentCardDto[] {
    const active = this.store.currentAgent;
    const detail = this.store.currentHostDetail;
    if (active === null || detail === null || active.workspace_id.length === 0) {
      return [];
    }
    const peers: AgentCardDto[] = [];
    for (let i = 0; i < detail.agents.length; i++) {
      const cand = detail.agents[i];
      if (
        cand.agent_id !== active.agent_id &&
        cand.workspace_id === active.workspace_id &&
        (cand.archived_at_rfc3339 === undefined || cand.archived_at_rfc3339 === null || cand.archived_at_rfc3339.length === 0)
      ) {
        peers.push(cand);
      }
    }
    return peers;
  }

  private hasParentAgent(): boolean {
    const active = this.store.currentAgent;
    return active !== null && active.parent_agent_id !== null && active.parent_agent_id.length > 0;
  }

  private formatUsagePill(): string {
    const usage = this.store.activeSessionUsage;
    if (usage === null) {
      return '';
    }
    const totalTok = (usage.input_tokens ?? 0) + (usage.output_tokens ?? 0);
    const parts: string[] = [];
    if (totalTok >= 1000) {
      parts.push(`${(totalTok / 1000).toFixed(1)}k tok`);
    } else if (totalTok > 0) {
      parts.push(`${totalTok} tok`);
    }
    if (usage.total_cost_usd !== undefined && usage.total_cost_usd !== null && usage.total_cost_usd > 0) {
      parts.push(`$${usage.total_cost_usd.toFixed(3)}`);
    }
    if (usage.context_percent !== undefined && usage.context_percent !== null && usage.context_percent > 0) {
      parts.push(`${Math.round(usage.context_percent)}% ctx`);
    }
    return parts.join(' · ');
  }

  private showLineageStrip(): boolean {
    return this.hasParentAgent() ||
      this.store.stageSubagents.length > 0 ||
      this.workspacePeers.length > 0 ||
      this.formatUsagePill().length > 0;
  }

  private subagentStatusColor(status: string): ResourceColor {
    const s = status.toLowerCase();
    if (s === 'running' || s === 'working' || s === 'streaming') {
      return this.store.palette.accentPrimary;
    }
    if (s === 'failed' || s === 'error') {
      return this.store.palette.statusDanger;
    }
    if (s === 'completed' || s === 'done' || s === 'succeeded') {
      return this.store.palette.statusDirect;
    }
    return this.store.palette.fgMuted;
  }

  @Builder
  private renderOfflineBanner(host: HostCardDto) {
    Row({ space: OrbitTokens.SPACE_SM }) {
      Row()
        .width(7)
        .height(7)
        .borderRadius(3.5)
        .backgroundColor(resolveHostStatusColor(host, this.store))

      Column({ space: 2 }) {
        Text(
          host.connection_state === 'connecting'
            ? $r('app.string.stage_reconnecting_banner')
            : $r('app.string.stage_offline_banner')
        )
          .attributeModifier(orbitTextMeta(this.store.palette.fgPrimary, FontWeight.Medium))
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })

        if (host.last_error !== null && host.last_error.trim().length > 0) {
          Text(host.last_error.trim())
            .attributeModifier(orbitTextMeta(this.store.palette.statusDanger))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
        }
      }
      .alignItems(HorizontalAlign.Start)
      .layoutWeight(1)

      Button($r('app.string.action_reconnect'), { type: ButtonType.Capsule, stateEffect: true })
        .attributeModifier(
          orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.listCard)
        )
        .constraintSize({ minHeight: 30 })
        .padding({ left: 10, right: 10 })
        .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.borderDefault })
        .onClick(() => {
          this.store.reconnectHost(host.host_id);
        })
    }
    .width('100%')
    .constraintSize({ maxWidth: OrbitTokens.STAGE_MAX_READING_WIDTH })
    .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD, top: 8, bottom: 8 })
    .backgroundColor(
      host.connection_state === 'connecting'
        ? this.store.palette.statusWarningSoft
        : this.store.palette.statusDangerSoft
    )
    .borderRadius(OrbitTokens.RADIUS_MD)
    .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
    .alignItems(VerticalAlign.Center)
  }

  private findCrewLiveAgent(hostId: string, agentId: string | null | undefined): AgentCardDto | null {
    if (agentId === null || agentId === undefined || agentId.length === 0) {
      return null;
    }
    const cleanHost = hostId.trim().length > 0 ? hostId.trim() : this.store.selectedHostId;
    const detail = cleanHost.length > 0
      ? this.store.fleetStore.getCachedHostDetail(cleanHost)
      : this.store.currentHostDetail;
    if (detail === null) {
      return null;
    }
    for (let i = 0; i < detail.agents.length; i += 1) {
      if (detail.agents[i].agent_id === agentId) {
        return detail.agents[i];
      }
    }
    return null;
  }

  private resolveCrewSubagentAgentSource(sub: SessionSubagentEntryDto): AgentCardDto | null {
    const targetHostId = resolveCrewSubagentHostId(sub, this.store.selectedHostId);
    const liveChild = this.findCrewLiveAgent(targetHostId, sub.child_agent_id);
    if (liveChild !== null) {
      return liveChild;
    }
    // Managed child lives on another/uncached host: NEVER masquerade with the parent's model.
    if (sub.child_agent_id !== null && sub.child_agent_id.trim().length > 0) {
      return null;
    }
    // Provider-native subagent runs inside the parent's session/provider.
    return this.store.currentAgent;
  }

  private computeSubagentDepth(sub: SessionSubagentEntryDto): number {
    return resolveCrewSubagentDepth(
      sub,
      this.store.selectedAgentId,
      this.store.selectedHostId,
      (hid: string, aid: string): AgentCardDto | null => this.findCrewLiveAgent(hid, aid)
    );
  }

  @Computed
  get crewAvatarMap(): Map<string, OrbitSubagentAvatarAsset> {
    const seeds: StageCrewAvatarSeed[] = [];
    const seen = new Set<string>();
    const subs = this.store.stageSubagents;
    for (let i = 0; i < subs.length; i += 1) {
      const k = `sub:${subs[i].id}`;
      if (!seen.has(k)) {
        seen.add(k);
        seeds.push({ key: k, seed: subs[i].child_agent_id ?? subs[i].id });
      }
    }
    const peers = this.workspacePeers;
    for (let i = 0; i < peers.length; i += 1) {
      const k = `peer:${peers[i].agent_id}`;
      if (!seen.has(k)) {
        seen.add(k);
        seeds.push({ key: k, seed: peers[i].agent_id });
      }
    }
    return assignSubagentCharacters(seeds, (item: StageCrewAvatarSeed): string => item.seed);
  }

  @Builder
  private renderLineageAndUsageStrip() {
    Scroll() {
      Row({ space: OrbitTokens.SPACE_XS }) {
        if (this.hasParentAgent()) {
          Button({ type: ButtonType.Capsule, stateEffect: true }) {
            Row({ space: 4 }) {
              SymbolGlyph($r('sys.symbol.chevron_left'))
                .fontSize(11)
                .fontColor([this.store.palette.accentPrimary])
              Text($r('app.string.stage_lineage_parent'))
                .attributeModifier(orbitTextMeta(this.store.palette.accentPrimary, FontWeight.Medium))
            }
            .alignItems(VerticalAlign.Center)
          }
          .constraintSize({ minHeight: 28 })
          .padding({ left: 8, right: 10 })
          .backgroundColor(this.store.palette.accentPrimarySoft)
          .onClick(() => {
            const parentId = this.store.currentAgent?.parent_agent_id ?? '';
            if (parentId.length > 0) {
              this.store.selectAgent(parentId);
            }
          })
        }

        ForEach(
          this.store.stageSubagents,
          (sub: SessionSubagentEntryDto) => {
            Column() {
              OrbitAgentListItem({
                variant: 'card',
                title: sub.role.trim().length > 0 ? sub.role.trim() : sub.id,
                status: mapCrewSubagentStatus(
                  sub,
                  this.findCrewLiveAgent(
                    resolveCrewSubagentHostId(sub, this.store.selectedHostId),
                    sub.child_agent_id
                  )
                ),
                characterId: this.crewAvatarMap.get(`sub:${sub.id}`)?.id ?? '',
                avatarSeedKey: sub.child_agent_id ?? sub.id,
                avatarSizeVp: 20,
                provider: this.resolveCrewSubagentAgentSource(sub)?.provider ?? '',
                modelLabel: resolveCompactAgentModelLabel(
                  this.resolveCrewSubagentAgentSource(sub),
                  this.store.providerCatalogsForHost(
                    resolveCrewSubagentHostId(sub, this.store.selectedHostId)
                  )
                ),
                effortLabel: resolveAgentEffortDisplay(
                  this.resolveCrewSubagentAgentSource(sub)
                ),
                agentLabel: resolveAgentFallbackLabel(
                  this.resolveCrewSubagentAgentSource(sub)
                ),
                secondaryMeta: (sub.host_name !== undefined && sub.host_name !== null && sub.host_name.trim().length > 0 &&
                  resolveCrewSubagentHostId(sub, this.store.selectedHostId) !== this.store.selectedHostId)
                  ? sub.host_name.trim()
                  : '',
                depth: this.computeSubagentDepth(sub),
                onItemClick: () => {
                  this.store.inspectSubagent(sub);
                }
              })
            }
            .constraintSize({ minWidth: 168, maxWidth: 236 })
          },
          (sub: SessionSubagentEntryDto) => `sub_${sub.id}_${sub.status}_${sub.depth ?? 0}_${sub.host_id ?? ''}`
        )

        ForEach(
          this.workspacePeers,
          (peer: AgentCardDto) => {
            Column() {
              OrbitAgentListItem({
                variant: 'card',
                title: peer.handle !== undefined && peer.handle !== null && peer.handle.length > 0
                  ? `@${peer.handle}`
                  : (peer.title.trim().length > 0 ? peer.title.trim() : peer.provider_label),
                status: mapAgentCardToListStatus(peer),
                characterId: this.crewAvatarMap.get(`peer:${peer.agent_id}`)?.id ?? '',
                avatarSeedKey: peer.agent_id,
                avatarSizeVp: 20,
                provider: peer.provider,
                modelLabel: resolveCompactAgentModelLabel(
                  peer,
                  this.store.providerCatalogsForHost(this.store.selectedHostId)
                ),
                effortLabel: resolveAgentEffortDisplay(peer),
                agentLabel: resolveAgentFallbackLabel(peer),
                depth: 0,
                onItemClick: () => {
                  this.store.selectAgent(peer.agent_id);
                }
              })
            }
            .constraintSize({ minWidth: 168, maxWidth: 236 })
          },
          (peer: AgentCardDto) => `peer_${peer.agent_id}_${peer.status.state}`
        )

        if (this.formatUsagePill().length > 0) {
          Text(this.formatUsagePill())
            .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
            .padding({ left: 8, right: 8, top: 4, bottom: 4 })
            .backgroundColor(this.store.palette.recessed)
            .borderRadius(999)
            .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
        }
      }
      .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
      .alignItems(VerticalAlign.Center)
    }
    .scrollable(ScrollDirection.Horizontal)
    .scrollBar(BarState.Off)
    .width('100%')
    .constraintSize({ maxWidth: OrbitTokens.STAGE_MAX_READING_WIDTH + OrbitTokens.SPACE_XL })
    .padding({ top: OrbitTokens.SPACE_XS, bottom: OrbitTokens.SPACE_XS })
  }

  @Builder
  private renderPairHostWelcome() {
    Column({ space: OrbitTokens.SPACE_LG }) {
      Blank()

      Column({ space: OrbitTokens.SPACE_SM }) {
        Text($r('app.string.stage_pair_first_host_title'))
          .attributeModifier(orbitTextTitle(this.store.palette.fgPrimary, FontWeight.Bold))
        Text($r('app.string.stage_pair_first_host_body'))
          .attributeModifier(orbitTextBody(this.store.palette.fgSecondary))
          .textAlign(TextAlign.Center)
      }
      .alignItems(HorizontalAlign.Center)
      .width('100%')

      Column({ space: OrbitTokens.SPACE_MD }) {
        Button($r('app.string.settings_scan_qr_code'))
          .attributeModifier(
            orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
          )
          .width('100%')
          .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
          .borderRadius(OrbitTokens.RADIUS_MD)
          .accessibilityText($r('app.string.settings_scan_qr_code'))
          .onClick(() => {
            this.store.scanQrAndPair(this.getUIContext().getHostContext());
          })

        Row({ space: OrbitTokens.SPACE_SM }) {
          TextInput({ placeholder: $r('app.string.stage_pair_uri_placeholder'), text: this.manualPairUri })
            .attributeModifier(
              orbitInputMono(
                this.store.palette.fgPrimary,
                this.store.palette.fgMuted,
                this.store.palette.code
              )
            )
            .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
            .borderRadius(OrbitTokens.RADIUS_MD)
            .border({
              width: OrbitTokens.HAIRLINE,
              color: this.store.palette.hairline
            })
            .layoutWeight(1)
            .accessibilityText($r('app.string.settings_pairing_uri_input_a11y'))
            .onChange((val: string) => {
              this.manualPairUri = val;
            })

          Button($r('app.string.settings_pair_host_submit'))
            .attributeModifier(
              orbitButtonSubhead(this.store.palette.fgPrimary, this.store.palette.subtleSurface)
            )
            .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
            .accessibilityText($r('app.string.settings_pair_host_submit'))
            .borderRadius(OrbitTokens.RADIUS_MD)
            .onClick(() => {
              dismissHarmonyKeyboard(this.getUIContext());
              if (this.manualPairUri.trim().length > 0) {
                this.store.pairHost(this.manualPairUri.trim());
                this.manualPairUri = '';
              }
            })
        }
        .width('100%')

        Button('Discover Local Daemon')
          .attributeModifier(orbitButtonSubhead(this.store.palette.fgSecondary, Color.Transparent))
          .height(OrbitTokens.HIT_MIN)
          .onClick(() => {
            this.store.discoverAndPairLocalDaemon();
          })
      }
      .constraintSize({ maxWidth: 460 })
      .width('100%')

      if (this.store.lastError.length > 0) {
        Text(this.store.lastError)
          .attributeModifier(orbitTextMeta(this.store.palette.statusDanger))
          .textAlign(TextAlign.Center)
      } else if (this.store.statusNote.length > 0) {
        Text(this.store.statusNote)
          .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
          .textAlign(TextAlign.Center)
      }

      Blank()
    }
    .width('100%')
    .height('100%')
    .padding({ left: OrbitTokens.SPACE_XL, right: OrbitTokens.SPACE_XL })
    .alignItems(HorizontalAlign.Center)
  }

  @Builder
  private renderProjectPickerMenu() {
    Menu() {
      MenuItemGroup({ header: 'Projects' }) {
        ForEach(
          this.launchpadProjects,
          (proj: LaunchpadProjectDto) => {
            MenuItem({
              symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.folder'))
                .fontColor([this.store.palette.iconPrimary]),
              content: proj.name,
              labelInfo: (proj.branch ?? '').length > 0 ? (proj.branch ?? undefined) : undefined,
              symbolEndIcon: cwdPathsEquivalent(proj.path, this.store.activeCwd)
                ? new SymbolGlyphModifier($r('sys.symbol.checkmark')).fontColor([this.store.palette.accentPrimary])
                : undefined
            })
              .labelFontColor(this.store.palette.fgMuted)
              .onClick(() => {
                this.store.setActiveCwd(proj.path);
              })
          },
          (proj: LaunchpadProjectDto) => proj.id
        )
      }

      MenuItemGroup() {
        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.folder_badge_plus'))
            .fontColor([this.store.palette.accentPrimary]),
          content: 'Browse Directories…'
        })
          .contentFontColor(this.store.palette.accentPrimary)
          .onClick(() => {
            this.store.browseDirectory(
              this.store.activeCwd === '~' ? null : this.store.activeCwd
            );
          })
      }
    }
  }

  @Builder
  private renderHostPickerMenu() {
    Menu() {
      MenuItemGroup({ header: 'Hosts' }) {
        ForEach(
          this.store.hosts,
          (host: HostCardDto) => {
            MenuItem({
              symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.dot_circle'))
                .fontColor([resolveHostStatusColor(host, this.store)]),
              content: host.display_name,
              labelInfo: this.hostSubtitle(host),
              symbolEndIcon: host.host_id === this.store.selectedHostId
                ? new SymbolGlyphModifier($r('sys.symbol.checkmark')).fontColor([this.store.palette.accentPrimary])
                : undefined
            })
              .labelFontColor(this.store.palette.fgMuted)
              .onClick(() => {
                this.store.selectHost(host.host_id);
                this.store.triggerHapticPreset('selection');
              })
          },
          (host: HostCardDto) => `stage_host_${host.host_id}`
        )
      }
    }
  }

  @Builder
  private renderExecutionModeMenu() {
    Menu() {
      MenuItemGroup({ header: 'Execution Environment' }) {
        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.pc'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Local',
          labelInfo: 'Current directory',
          symbolEndIcon: !this.store.draftUseWorktree
            ? new SymbolGlyphModifier($r('sys.symbol.checkmark')).fontColor([this.store.palette.accentPrimary])
            : undefined
        })
          .labelFontColor(this.store.palette.fgMuted)
          .onClick(() => {
            this.store.setDraftUseWorktree(false);
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.arrow_triangle_divide'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Worktree',
          labelInfo: 'Isolated git worktree',
          symbolEndIcon: this.store.draftUseWorktree
            ? new SymbolGlyphModifier($r('sys.symbol.checkmark')).fontColor([this.store.palette.accentPrimary])
            : undefined
        })
          .labelFontColor(this.store.palette.fgMuted)
          .onClick(() => {
            this.store.setDraftUseWorktree(true);
          })
      }
    }
  }

  @Builder
  private renderNewSessionWelcome() {
    Column({ space: 0 }) {
      Blank()

      Text($r('app.string.stage_welcome_prompt'))
        .attributeModifier(orbitTextTitle(this.store.palette.fgPrimary, FontWeight.Bold))
        .textAlign(TextAlign.Center)
        .maxLines(2)
        .width('100%')

      // Contextual Readiness Pills: Native HarmonyOS 7 Capsule Buttons + Native Menus
      Flex({
        wrap: FlexWrap.Wrap,
        justifyContent: FlexAlign.Center,
        alignItems: ItemAlign.Center
      }) {
        Button({ type: ButtonType.Capsule, stateEffect: true }) {
          Row({ space: 6 }) {
            SymbolGlyph($r('sys.symbol.folder'))
              .fontSize(14)
              .fontColor([this.store.palette.iconSecondary])
            Text(this.activeProjectLabel)
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
              .constraintSize({ maxWidth: 160 })
            SymbolGlyph($r('sys.symbol.chevron_down'))
              .fontSize(11)
              .fontColor([this.store.palette.fgMuted])
          }
          .alignItems(VerticalAlign.Center)
        }
        .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
        .padding({ left: 14, right: 12 })
        .margin({ left: 4, right: 4, top: 4, bottom: 4 })
        .backgroundColor(this.store.palette.subtleSurface)
        .accessibilityText(`Project: ${this.activeProjectLabel}`)
        .bindMenu(this.renderProjectPickerMenu())

        if (this.store.hosts.length > 1 && this.activeHostCard !== null) {
          Button({ type: ButtonType.Capsule, stateEffect: true }) {
            Row({ space: 6 }) {
              Row()
                .width(7)
                .height(7)
                .borderRadius(3.5)
                .backgroundColor(resolveHostStatusColor(this.activeHostCard, this.store))
              Text(this.activeHostCard.display_name)
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                .maxLines(1)
                .textOverflow({ overflow: TextOverflow.Ellipsis })
                .constraintSize({ maxWidth: 140 })
              SymbolGlyph($r('sys.symbol.chevron_down'))
                .fontSize(11)
                .fontColor([this.store.palette.fgMuted])
            }
            .alignItems(VerticalAlign.Center)
          }
          .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
          .padding({ left: 14, right: 12 })
          .margin({ left: 4, right: 4, top: 4, bottom: 4 })
          .backgroundColor(this.store.palette.subtleSurface)
          .accessibilityText(`Host: ${this.activeHostCard.display_name}`)
          .bindMenu(this.renderHostPickerMenu())
        }

        if (this.isDraftSession()) {
          Button({ type: ButtonType.Capsule, stateEffect: true }) {
            Row({ space: 6 }) {
              SymbolGlyph(
                this.store.draftUseWorktree
                  ? $r('sys.symbol.arrow_triangle_divide')
                  : $r('sys.symbol.pc')
              )
                .fontSize(14)
                .fontColor([this.store.palette.iconSecondary])
              Text(this.store.draftUseWorktree ? 'Worktree' : 'Local')
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                .maxLines(1)
              SymbolGlyph($r('sys.symbol.chevron_down'))
                .fontSize(11)
                .fontColor([this.store.palette.fgMuted])
            }
            .alignItems(VerticalAlign.Center)
          }
          .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
          .padding({ left: 14, right: 12 })
          .margin({ left: 4, right: 4, top: 4, bottom: 4 })
          .backgroundColor(this.store.palette.subtleSurface)
          .accessibilityText(
            this.store.draftUseWorktree
              ? $r('app.string.a11y_exec_env_worktree')
              : $r('app.string.a11y_exec_env_local')
          )
          .bindMenu(this.renderExecutionModeMenu())
        }
      }
      .width('100%')
      .margin({ top: OrbitTokens.SPACE_MD })

      if (this.activeHostCard !== null && !this.isHostOnline(this.activeHostCard)) {
        Row({ space: 6 }) {
          Row()
            .width(6)
            .height(6)
            .borderRadius(3)
            .backgroundColor(resolveHostStatusColor(this.activeHostCard, this.store))
          Text(
            `${formatCompactTopBarConnectionLabel(
              this.activeHostCard.connection_state,
              this.activeHostCard.via_ssh_tunnel
            )} · ${this.activeHostCard.display_name}`
          )
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
        }
        .margin({ top: OrbitTokens.SPACE_SM })
        .alignItems(VerticalAlign.Center)
      } else if (this.store.activeGitBranch().length > 0) {
        Row({ space: 4 }) {
          SymbolGlyph($r('sys.symbol.arrow_triangle_divide'))
            .fontSize(12)
            .fontColor([this.store.palette.fgMuted])
          Text(
            this.store.draftUseWorktree
              ? `Worktree from ${this.store.activeGitBranch()}`
              : this.store.activeGitBranch()
          )
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
        }
        .margin({ top: OrbitTokens.SPACE_SM })
        .alignItems(VerticalAlign.Center)
      }

      if (this.store.lastError.length > 0) {
        Text(this.store.lastError)
          .attributeModifier(orbitTextMeta(this.store.palette.statusDanger))
          .textAlign(TextAlign.Center)
          .margin({ top: OrbitTokens.SPACE_MD })
      }

      Blank()
    }
    .width('100%')
    .height('100%')
    .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG, bottom: OrbitTokens.SPACE_XL })
    .alignItems(HorizontalAlign.Center)
  }

  build() {
    if (this.store.hosts.length === 0) {
      this.renderPairHostWelcome()
    } else if (this.store.stageTurns.length === 0) {
      Refresh({ refreshing: $$this.isRefreshing }) {
        this.renderNewSessionWelcome()
      }
      .onRefreshing(() => {
        this.handlePullRefresh();
      })
      .width('100%')
      .height('100%')
    } else {
      Stack({ alignContent: Alignment.Bottom }) {
        Column({ space: 0 }) {
          if (this.activeHostCard !== null && !this.isHostOnline(this.activeHostCard)) {
            Column() {
              this.renderOfflineBanner(this.activeHostCard)
            }
            .width('100%')
            .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG, top: OrbitTokens.SPACE_SM })
            .alignItems(HorizontalAlign.Center)
          }

          if (this.showLineageStrip()) {
            Column() {
              this.renderLineageAndUsageStrip()
            }
            .width('100%')
            .alignItems(HorizontalAlign.Center)
          }

          Refresh({ refreshing: $$this.isRefreshing }) {
            List({
              space: OrbitTokens.SPACE_XL,
              initialIndex: Math.max(0, this.store.stageTurns.length - 1),
              scroller: this.listScroller
            }) {
              if (this.store.stageHasOlderHistory) {
                ListItem() {
                  Row() {
                    if (this.store.stageLoadingOlder) {
                      Row({ space: OrbitTokens.SPACE_SM }) {
                        LoadingProgress()
                          .width(14)
                          .height(14)
                          .color(this.store.palette.fgMuted)
                        Text($r('app.string.stage_loading_older'))
                          .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                      }
                      .alignItems(VerticalAlign.Center)
                    } else {
                      Button($r('app.string.stage_load_older'), { type: ButtonType.Capsule, stateEffect: true })
                        .attributeModifier(
                          orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface)
                        )
                        .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                        .padding({ left: 14, right: 14 })
                        .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
                        .onClick(() => {
                          this.store.loadOlderHistory();
                        })
                    }
                  }
                  .width('100%')
                  .justifyContent(FlexAlign.Center)
                  .padding({ bottom: OrbitTokens.SPACE_XS })
                }
              }

              Repeat<OrbitStageTurnModel>(this.store.stageTurns)
                .each((ri: RepeatItem<OrbitStageTurnModel>) => {
                  ListItem() {
                    Column() {
                      StageTurnView({
                        turn: ri.item,
                        isTurnActive:
                          (this.store.stageStore.activeTurnKey.length > 0 &&
                            ri.item.turnKey === this.store.stageStore.activeTurnKey) ||
                          ri.item.hasActiveItem ||
                          (this.store.isActiveAgentRunning() && ri.index === this.store.stageTurns.length - 1)
                      })
                        .constraintSize({ maxWidth: OrbitTokens.STAGE_MAX_READING_WIDTH })
                    }
                    .width('100%')
                    .alignItems(HorizontalAlign.Center)
                  }
                })
                .key((t: OrbitStageTurnModel): string => t.turnKey)
                .virtualScroll()
            }
            .width('100%')
            .height('100%')
            .padding({
              left: OrbitTokens.SPACE_LG,
              right: OrbitTokens.SPACE_LG,
              top: OrbitTokens.SPACE_LG,
              bottom: OrbitTokens.SPACE_XL
            })
            .scrollBar(BarState.Auto)
            .edgeEffect(EdgeEffect.Spring)
            .onReachStart(() => {
              if (
                this.store.stageHasOlderHistory &&
                !this.store.stageLoadingOlder &&
                this.store.stageTurns.length > 0
              ) {
                this.store.loadOlderHistory();
              }
            })
            .onScrollStart(() => {
              if (this.store.keyboardHeightVp > 0) {
                dismissHarmonyKeyboard(this.getUIContext());
              }
            })
            .onScrollIndex((firstIndex: number, endIndex: number) => {
              const lastIdx = this.lastListItemIndex();
              this.firstVisibleIndex = firstIndex;
              this.lastVisibleIndex = endIndex;
              if (!this.isAutoAnchoringResize && this.store.stageTurns.length > 0 && endIndex < lastIdx) {
                this.atBottomEdge = false;
              }
            })
            .onReachEnd(() => {
              this.atBottomEdge = true;
              this.lastVisibleIndex = this.lastListItemIndex();
            })
            .onScrollFrameBegin((offset: number, _state: ScrollState) => {
              if (offset < -2 && this.store.stageTurns.length > 0) {
                this.atBottomEdge = false;
              }
              return { offsetRemain: offset };
            })
          }
          .onRefreshing(() => {
            this.handlePullRefresh();
          })
          .width('100%')
          .layoutWeight(1)
        }
        .width('100%')
        .height('100%')

        if (this.showJumpToLatest()) {
          Badge({
            count: this.store.isActiveAgentRunning() ? 1 : 0,
            style: {
              badgeColor: this.store.palette.accentPrimaryFill,
              badgeSize: 8
            },
            position: BadgePosition.RightTop
          }) {
            Button({ type: ButtonType.Circle, stateEffect: true }) {
              SymbolGlyph($r('sys.symbol.arrow_down'))
                .fontSize(18)
                .renderingStrategy(SymbolRenderingStrategy.SINGLE)
                .fontColor([this.store.palette.iconPrimary])
            }
            .width(OrbitTokens.HIT_COMPACT)
            .height(OrbitTokens.HIT_COMPACT)
            .padding(0)
            .attributeModifier(
              orbitSurface(
                this.store.palette.listCard,
                this.store.palette.hairline,
                OrbitTokens.HAIRLINE,
                ShadowStyle.OUTER_FLOATING_SM
              )
            )
            .accessibilityText(
              this.store.isActiveAgentRunning()
                ? $r('app.string.a11y_scroll_latest_streaming')
                : $r('app.string.a11y_scroll_latest_message')
            )
            .onClick(() => {
              const lastIdx = this.lastListItemIndex();
              this.atBottomEdge = true;
              this.lastVisibleIndex = lastIdx;
              this.listScroller.scrollToIndex(lastIdx, true, ScrollAlign.END);
              this.listScroller.scrollEdge(Edge.Bottom);
              this.store.triggerHaptic('selection');
            })
          }
          .margin({ bottom: OrbitTokens.SPACE_SM })
        }
      }
      .width('100%')
      .height('100%')
      .onAreaChange((oldArea: Area, newArea: Area) => {
        const prevH = Number(oldArea.height);
        const nextH = Number(newArea.height);
        if (
          prevH > 0 &&
          nextH > 0 &&
          nextH < prevH - 4 &&
          this.atBottomEdge &&
          this.store.stageTurns.length > 0
        ) {
          const lastIdx = this.lastListItemIndex();
          this.isAutoAnchoringResize = true;
          this.lastVisibleIndex = lastIdx;
          this.listScroller.scrollToIndex(lastIdx, true, ScrollAlign.END);
          this.listScroller.scrollEdge(Edge.Bottom);
          if (this.resizeTimerId !== -1) {
            clearTimeout(this.resizeTimerId);
          }
          this.resizeTimerId = setTimeout(() => {
            if (this.atBottomEdge && this.store.stageTurns.length > 0) {
              this.listScroller.scrollEdge(Edge.Bottom);
            }
            this.isAutoAnchoringResize = false;
            this.resizeTimerId = -1;
          }, 80);
        }
      })
    }
  }
}
