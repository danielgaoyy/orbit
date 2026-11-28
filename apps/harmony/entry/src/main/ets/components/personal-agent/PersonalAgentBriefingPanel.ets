import { LengthMetrics } from '@kit.ArkUI';
import {
  AgentCardDto,
  BriefingDecisionDto,
  BriefingHostDto,
  BriefingWaitItemDto,
  PersonalAgentActivityEntryDto,
  PersonalAgentBriefingDto,
  formatProviderLabel
} from '../../model/OrbitTypes';
import { orbitRuntime } from '../../model/OrbitRuntime';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { OrbitFleetStore } from '../../model/stores/OrbitFleetStore';
import { OrbitPersonalAgentStore } from '../../model/stores/OrbitPersonalAgentStore';
import { OrbitTheme } from '../../theme/OrbitTheme';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import {
  OrbitAgentListItem,
  resolveAgentEffortDisplay,
  resolveAgentFallbackLabel,
  resolveCompactAgentModelLabel
} from '../common/OrbitAgentListItem';
import {
  OrbitAvatarSeedKeyed,
  OrbitSubagentAvatarAsset,
  assignSubagentCharacters
} from '../common/OrbitSubagentAvatar';
import {
  FrozenBatchApprovalTarget,
  InProgressPaginationResult,
  PartitionedBriefingWorker,
  PartitionedBriefingWorkers,
  PersonalAgentIdentityRef,
  SinceLastVisitSummary,
  briefingWorkerKey,
  computeSinceLastVisitSummary,
  filterActivityForPersonalAgent,
  formatBriefingElapsedResource,
  formatWaitCountdownResource,
  paginateInProgressWorkers,
  partitionBriefingWorkers,
  selectConfirmedArchivableWorkers,
  snapshotBatchApprovalTargets
} from './PersonalAgentBriefingModel';
import { OrbitPullRefreshController } from '../settings/OrbitSettingsNavModel';

const DEFAULT_VISIBLE_WORKERS: number = 5;
const DEFAULT_VISIBLE_WAITING: number = 3;
const DEFAULT_VISIBLE_ACTIVITY: number = 5;

interface BriefingAvatarSeedItem extends OrbitAvatarSeedKeyed {
  readonly key: string;
  readonly agentId: string;
}

@ComponentV2
export struct PersonalAgentBriefingPanel {
  @Param embedded: boolean = false;
  @Event onClose: () => void = () => {};

  @Local private awayBannerDismissed: boolean = false;
  @Local private showAllInProgress: boolean = false;
  @Local private showAllWaiting: boolean = false;
  @Local private showAllCompleted: boolean = false;
  @Local private showAllActivity: boolean = false;
  @Local private collapsedNeedsYou: boolean = false;
  @Local private collapsedInProgress: boolean = false;
  @Local private collapsedWaiting: boolean = false;
  @Local private collapsedCompleted: boolean = false;
  @Local private collapsedHandled: boolean = false;
  @Local private collapsedHosts: boolean = false;
  @Local private expandedActivityKeys: string[] = [];
  @Local private isRefreshing: boolean = false;
  @Local private batchApproving: boolean = false;

  private readonly store: OrbitHarmonyStore = orbitRuntime();
  @Local private fleetStore: OrbitFleetStore = orbitRuntime().fleetStore;
  @Local private personalAgentStore: OrbitPersonalAgentStore = orbitRuntime().personalAgentStore;
  private readonly pullRefreshCtrl: OrbitPullRefreshController = new OrbitPullRefreshController();
  private refreshTimerId: number = -1;

  aboutToAppear(): void {
    // Reuse fresh briefing without scheduling a trailing duplicate RPC when entering PA + mounting panel.
    this.store.ensureFreshPersonalAgentBriefing();
    const pa = this.personalAgentRef;
    if (pa !== null) {
      this.store.refreshPersonalAgentActivity(pa.hostId, 50, null, false);
    } else if (this.store.primaryRouterHostId.length > 0) {
      this.store.refreshPersonalAgentActivity(this.store.primaryRouterHostId, 50, null, false);
    }
  }

  aboutToDisappear(): void {
    // Presentation container unmount (e.g., resizing 1023 <-> 1024 vp while staying in the same PA)
    // MUST ONLY clean up local component timers and NEVER overwrite `sinceLastVisitMs`.
    this.pullRefreshCtrl.unmount();
    this.isRefreshing = false;
    if (this.refreshTimerId !== -1) {
      clearTimeout(this.refreshTimerId);
      this.refreshTimerId = -1;
    }
  }

  @Monitor('fleetStore.selectedHostId', 'personalAgentStore.primaryRouterHostId')
  onBriefingScopeChanged(_mon: IMonitor): void {
    const scope = `${this.fleetStore.selectedHostId}::${this.personalAgentStore.primaryRouterHostId}`;
    if (this.pullRefreshCtrl.onScopeChanged(scope)) {
      this.isRefreshing = false;
    }
  }

  @Computed
  get briefingData(): PersonalAgentBriefingDto | null {
    return this.store.briefing;
  }

  @Computed
  get personalAgentRef(): PersonalAgentIdentityRef | null {
    const pa = this.briefingData?.personal_agent ?? null;
    if (pa !== null && pa.host_id.length > 0 && pa.agent_id.length > 0) {
      return { hostId: pa.host_id, agentId: pa.agent_id };
    }
    if (this.store.selectedHostId.length > 0 && this.store.selectedAgentId.length > 0) {
      return { hostId: this.store.selectedHostId, agentId: this.store.selectedAgentId };
    }
    return null;
  }

  @Computed
  get needsYouItems(): BriefingDecisionDto[] {
    return this.briefingData?.needs_you ?? [];
  }

  @Computed
  get batchEligibleItems(): BriefingDecisionDto[] {
    const out: BriefingDecisionDto[] = [];
    const list = this.needsYouItems;
    for (let i = 0; i < list.length; i += 1) {
      const item = list[i];
      if (
        item.batch_eligible &&
        !item.human_only &&
        item.kind === 'permission' &&
        item.request_id !== null &&
        item.request_id.length > 0
      ) {
        out.push(item);
      }
    }
    return out;
  }

  @Computed
  get partitionedWorkers(): PartitionedBriefingWorkers {
    const b = this.briefingData;
    return partitionBriefingWorkers(
      b !== null ? b.missions : [],
      b !== null ? b.other_active : [],
      this.needsYouItems,
      this.store.fleetStore.getAllCachedHostDetails(),
      b !== null ? (b.waiting ?? []) : []
    );
  }

  @Computed
  get inProgressPage(): InProgressPaginationResult {
    return paginateInProgressWorkers(
      this.partitionedWorkers.runningMissionWorkers,
      this.partitionedWorkers.runningOtherWorkers,
      this.showAllInProgress,
      DEFAULT_VISIBLE_WORKERS
    );
  }

  @Computed
  get waitingItems(): BriefingWaitItemDto[] {
    return this.briefingData?.waiting ?? [];
  }

  @Computed
  get hostItems(): BriefingHostDto[] {
    return this.briefingData?.hosts ?? [];
  }

  @Computed
  get filteredActivity(): PersonalAgentActivityEntryDto[] {
    return filterActivityForPersonalAgent(
      this.store.personalAgentActivity,
      this.briefingData,
      this.personalAgentRef
    );
  }

  @Computed
  get sinceLastVisit(): SinceLastVisitSummary {
    return computeSinceLastVisitSummary(
      this.briefingData,
      this.filteredActivity,
      this.store.personalAgentStore.sinceLastVisitMs
    );
  }

  @Computed
  get characterByKey(): Map<string, OrbitSubagentAvatarAsset> {
    const seen = new Set<string>();
    const seeds: BriefingAvatarSeedItem[] = [];
    const pushSeed = (hostId: string, agentId: string): void => {
      const k = briefingWorkerKey(hostId, agentId);
      if (!seen.has(k)) {
        seen.add(k);
        seeds.push({ key: k, agentId: agentId });
      }
    };
    for (let i = 0; i < this.needsYouItems.length; i += 1) {
      pushSeed(this.needsYouItems[i].host_id, this.needsYouItems[i].agent_id);
    }
    const b = this.briefingData;
    if (b !== null) {
      for (let i = 0; i < b.missions.length; i += 1) {
        const m = b.missions[i];
        pushSeed(m.root.host_id, m.root.agent_id);
        for (let j = 0; j < m.workers.length; j += 1) {
          pushSeed(m.workers[j].host_id, m.workers[j].agent_id);
        }
      }
      for (let i = 0; i < b.other_active.length; i += 1) {
        pushSeed(b.other_active[i].host_id, b.other_active[i].agent_id);
      }
    }
    return assignSubagentCharacters(
      seeds,
      (entry: BriefingAvatarSeedItem): string => entry.agentId
    );
  }

  private resolveCharacterId(hostId: string, agentId: string): string {
    const asset = this.characterByKey.get(briefingWorkerKey(hostId, agentId));
    return asset !== undefined ? asset.id : '';
  }

  private resolveLiveAgent(hostId: string, agentId: string): AgentCardDto | null {
    const detail = this.store.fleetStore.getCachedHostDetail(hostId);
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

  private resolveModelLabel(hostId: string, agent: AgentCardDto | null): string {
    const detail = this.store.fleetStore.getCachedHostDetail(hostId);
    const catalogs = detail !== null ? detail.provider_catalogs : [];
    return resolveCompactAgentModelLabel(agent, catalogs);
  }

  private isHighRisk(riskLevel: string | null): boolean {
    if (riskLevel === null) {
      return false;
    }
    const lower = riskLevel.toLowerCase();
    return lower === 'high' || lower === 'critical';
  }

  private riskBadgeResource(riskLevel: string | null): Resource {
    const lower = (riskLevel ?? '').toLowerCase();
    if (lower === 'critical') {
      return $r('app.string.approval_criticalRisk');
    }
    if (lower === 'high') {
      return $r('app.string.approval_highRisk');
    }
    if (lower === 'medium') {
      return $r('app.string.approval_mediumRisk');
    }
    return $r('app.string.briefing_lowRisk');
  }

  private riskBadgeTextColor(riskLevel: string | null): ResourceColor {
    const lower = (riskLevel ?? '').toLowerCase();
    if (lower === 'critical' || lower === 'high') {
      return this.store.palette.statusDanger;
    }
    if (lower === 'medium') {
      return this.store.palette.statusWarning;
    }
    return this.store.palette.statusDirect;
  }

  private riskBadgeBgColor(riskLevel: string | null): ResourceColor {
    const lower = (riskLevel ?? '').toLowerCase();
    if (lower === 'critical' || lower === 'high') {
      return this.store.palette.statusDangerSoft;
    }
    if (lower === 'medium') {
      return this.store.palette.statusWarningSoft;
    }
    return this.store.palette.statusDirectSoft;
  }

  private actorLabelResource(actor: string): Resource {
    if (actor === 'personal_agent') {
      return $r('app.string.briefing_actorPersonalAgent');
    }
    if (actor === 'guardian_tier1') {
      return $r('app.string.briefing_actorGuardian');
    }
    return $r('app.string.briefing_actorRule');
  }

  private isActivityExpanded(key: string): boolean {
    return this.expandedActivityKeys.indexOf(key) >= 0;
  }

  private toggleActivityExpanded(key: string): void {
    if (this.isActivityExpanded(key)) {
      this.expandedActivityKeys = this.expandedActivityKeys.filter((k: string): boolean => k !== key);
    } else {
      this.expandedActivityKeys = this.expandedActivityKeys.concat([key]);
    }
  }

  private handleRefresh(): void {
    const scope = `${this.store.selectedHostId}::${this.store.primaryRouterHostId}`;
    const pa = this.personalAgentRef;
    this.store.triggerHaptic('selection');
    void this.pullRefreshCtrl.run(
      scope,
      async (): Promise<void> => {
        this.store.refreshPersonalAgentBriefing(true);
        if (pa !== null) {
          this.store.refreshPersonalAgentActivity(pa.hostId, 50, null, true);
        } else if (this.store.primaryRouterHostId.length > 0) {
          this.store.refreshPersonalAgentActivity(this.store.primaryRouterHostId, 50, null, true);
        }
        await this.store.pullRefreshFleetAsync();
      },
      (refreshing: boolean): void => {
        this.isRefreshing = refreshing;
      }
    );
  }

  private confirmBatchApprove(): void {
    const frozenTargets: FrozenBatchApprovalTarget[] = snapshotBatchApprovalTargets(this.batchEligibleItems);
    const count = frozenTargets.length;
    if (count <= 0 || this.batchApproving) {
      return;
    }
    this.getUIContext().showAlertDialog({
      title: $r('app.string.briefing_confirmBatchTitle', count),
      message: $r('app.string.briefing_confirmBatchMessage'),
      autoCancel: true,
      primaryButton: {
        value: $r('app.string.common_cancel'),
        fontColor: this.store.palette.fgSecondary,
        action: () => {
          // User canceled batch approval
        }
      },
      secondaryButton: {
        value: $r('app.string.common_confirm'),
        fontColor: this.store.palette.accentPrimary,
        action: () => {
          this.batchApproving = true;
          this.store.approveSafeBatchPermissions(frozenTargets).finally((): void => {
            this.batchApproving = false;
          });
        }
      }
    });
  }

  private openWorkerSession(
    hostId: string,
    agentId: string,
    eventKind: string = '',
    requestId: string = ''
  ): void {
    if (!this.embedded) {
      this.onClose();
    }
    this.store.openSessionFromDeepLink(hostId, agentId, eventKind, requestId, 0);
  }

  private async openDecisionDiffPreview(decision: BriefingDecisionDto): Promise<void> {
    let liveAg = this.resolveLiveAgent(decision.host_id, decision.agent_id);
    if (liveAg === null && decision.host_id.length > 0) {
      await this.store.fleetStore.reloadHostAsync(decision.host_id);
      liveAg = this.resolveLiveAgent(decision.host_id, decision.agent_id);
    }
    const targetCwd = liveAg !== null ? liveAg.cwd : '';
    const trimmedCwdCheck = targetCwd.trim();
    if (trimmedCwdCheck.length === 0 || trimmedCwdCheck === '~') {
      this.store.showLocalizedToast(
        $r('app.string.briefing_diffTargetUnavailable'),
        'Workspace path unavailable for this request',
        true
      );
      return;
    }
    if (!this.embedded) {
      this.onClose();
    }
    this.store.openDecisionDiffForHostCwd(decision.host_id, targetCwd);
  }

  private handleArchiveDone(): void {
    const verified = selectConfirmedArchivableWorkers(
      this.partitionedWorkers.archivableCompletedWorkers,
      this.store.briefing,
      this.store.fleetStore.getAllCachedHostDetails()
    );
    for (let i = 0; i < verified.length; i += 1) {
      const item = verified[i];
      this.store.archiveAgentOnHost(item.worker.host_id, item.worker.agent_id);
    }
    if (verified.length > 0) {
      this.store.refreshPersonalAgentBriefing(true);
    }
  }

  @Builder
  private renderAwayBanner() {
    if (!this.awayBannerDismissed && this.sinceLastVisit.hasSummary) {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Image($r('app.media.orbit_agent_bot'))
          .width(14)
          .height(14)
          .objectFit(ImageFit.Contain)
          .fillColor($r('app.color.icon_primary'))
          .draggable(false)
          .flexShrink(0)

        Text(
          this.sinceLastVisit.completed > 0 && this.sinceLastVisit.autoHandled > 0
            ? $r(
              'app.string.briefing_whileAwayBoth',
              this.sinceLastVisit.completed,
              this.sinceLastVisit.autoHandled
            )
            : (this.sinceLastVisit.completed > 0
              ? $r('app.string.briefing_whileAwayCompletedOnly', this.sinceLastVisit.completed)
              : $r('app.string.briefing_whileAwayHandledOnly', this.sinceLastVisit.autoHandled))
        )
          .attributeModifier(orbitTextMeta(this.store.palette.fgPrimary, FontWeight.Medium))
          .layoutWeight(1)

        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.xmark'))
            .fontSize(11)
            .fontColor([this.store.palette.fgMuted])
        }
        .width(26)
        .height(26)
        .padding(0)
        .backgroundColor(Color.Transparent)
        .accessibilityText($r('app.string.common_close'))
        .onClick(() => {
          this.awayBannerDismissed = true;
          const pa = this.personalAgentRef;
          if (pa !== null) {
            this.store.markPersonalAgentVisitedNow(pa.hostId, pa.agentId);
          }
        })
      }
      .width('100%')
      .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_SM, top: 8, bottom: 8 })
      .borderRadius(OrbitTokens.RADIUS_MD)
      .attributeModifier(OrbitTheme.recessedWell(this.store.palette))
      .alignItems(VerticalAlign.Center)
    }
  }

  @Builder
  private renderDecisionFooter(decision: BriefingDecisionDto) {
    Column({ space: OrbitTokens.SPACE_SM }) {
      if (decision.risk_level !== null && decision.risk_level.trim().length > 0) {
        Row({ space: 6 }) {
          Text(this.riskBadgeResource(decision.risk_level))
            .attributeModifier(
              orbitTextMeta(this.riskBadgeTextColor(decision.risk_level), FontWeight.Medium)
            )
            .padding({ left: 7, right: 7, top: 2, bottom: 2 })
            .borderRadius(OrbitTokens.RADIUS_XS)
            .backgroundColor(this.riskBadgeBgColor(decision.risk_level))
        }
        .width('100%')
      }

      if (decision.summary.trim().length > 0) {
        Text(decision.summary.trim())
          .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
          .maxLines(2)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .width('100%')
      }

      if (decision.router_recommendation !== null) {
        Row({ space: 6 }) {
          Image($r('app.media.orbit_agent_bot'))
            .width(12)
            .height(12)
            .objectFit(ImageFit.Contain)
            .fillColor($r('app.color.icon_primary'))
            .draggable(false)
            .flexShrink(0)

          Column({ space: 2 }) {
            Text(
              decision.router_recommendation.decision === 'deny'
                ? $r('app.string.briefing_paRecommendDeny')
                : $r('app.string.briefing_paRecommendAllow')
            )
              .attributeModifier(
                orbitTextMeta(
                  decision.router_recommendation.decision === 'deny'
                    ? this.store.palette.statusDanger
                    : this.store.palette.statusDirect,
                  FontWeight.Medium
                )
              )
            if (
              decision.router_recommendation.reason !== undefined &&
              decision.router_recommendation.reason !== null &&
              decision.router_recommendation.reason.trim().length > 0
            ) {
              Text(decision.router_recommendation.reason.trim())
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                .maxLines(2)
                .textOverflow({ overflow: TextOverflow.Ellipsis })
            }
          }
          .alignItems(HorizontalAlign.Start)
          .layoutWeight(1)
        }
        .width('100%')
        .padding(OrbitTokens.SPACE_SM)
        .borderRadius(OrbitTokens.RADIUS_SM)
        .attributeModifier(OrbitTheme.recessedWell(this.store.palette))
        .alignItems(VerticalAlign.Top)
      }

      // Action strip: Diff preview + Deny + Allow + Open
      Flex({
        wrap: FlexWrap.Wrap,
        justifyContent: FlexAlign.End,
        space: { main: LengthMetrics.vp(6), cross: LengthMetrics.vp(6) }
      }) {
        Button($r('app.string.briefing_openDiffPreview'), { type: ButtonType.Capsule, stateEffect: true })
          .attributeModifier(
            orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface)
          )
          .constraintSize({ minHeight: 30 })
          .padding({ left: 10, right: 10 })
          .onClick(() => {
            this.openDecisionDiffPreview(decision);
          })

        if (
          decision.kind === 'permission' &&
          decision.request_id !== null &&
          decision.request_id.length > 0
        ) {
          Button($r('app.string.approval_deny'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(
              orbitButtonMeta(this.store.palette.statusDanger, this.store.palette.statusDangerSoft)
            )
            .constraintSize({ minHeight: 30 })
            .padding({ left: 12, right: 12 })
            .onClick(() => {
              this.store.resolveBriefingDecision(decision, 'deny');
            })

          Button($r('app.string.approval_allow'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(
              orbitButtonMeta(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
            )
            .constraintSize({ minHeight: 30 })
            .padding({ left: 12, right: 12 })
            .onClick(() => {
              this.store.resolveBriefingDecision(decision, 'allow');
            })
        }

        Button(
          decision.kind === 'plan'
            ? $r('app.string.briefing_reviewAction')
            : $r('app.string.briefing_viewAction'),
          { type: ButtonType.Capsule, stateEffect: true }
        )
          .attributeModifier(
            orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface)
          )
          .constraintSize({ minHeight: 30 })
          .padding({ left: 10, right: 10 })
          .onClick(() => {
            this.openWorkerSession(
              decision.host_id,
              decision.agent_id,
              decision.kind === 'permission' ? 'permission_required' : '',
              decision.request_id ?? ''
            );
          })
      }
      .width('100%')
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  @Builder
  private renderNeedsYouSection() {
    if (this.needsYouItems.length > 0) {
      Column({ space: OrbitTokens.SPACE_SM }) {
        Row({ space: 6 }) {
          Row()
            .width(OrbitTokens.DOT_SIZE)
            .height(OrbitTokens.DOT_SIZE)
            .borderRadius(3)
            .backgroundColor(this.store.palette.statusWarning)
            .flexShrink(0)

          Text($r('app.string.briefing_needsYouTitle'))
            .attributeModifier(orbitTextMeta(this.store.palette.statusWarning, FontWeight.Medium))
            .maxLines(1)

          Text(`${this.needsYouItems.length}`)
            .attributeModifier(orbitTextMono(this.store.palette.statusWarning, FontWeight.Medium))
            .maxLines(1)

          Blank()

          if (this.batchEligibleItems.length > 0) {
            Button(
              $r('app.string.briefing_approveAll', this.batchEligibleItems.length),
              { type: ButtonType.Capsule, stateEffect: true }
            )
              .attributeModifier(
                orbitButtonMeta(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
              )
              .constraintSize({ minHeight: 28 })
              .padding({ left: 10, right: 10 })
              .enabled(!this.batchApproving)
              .onClick(() => {
                this.confirmBatchApprove();
              })
          }

          Button({ type: ButtonType.Circle, stateEffect: true }) {
            SymbolGlyph(
              this.collapsedNeedsYou ? $r('sys.symbol.chevron_right') : $r('sys.symbol.chevron_down')
            )
              .fontSize(11)
              .fontColor([this.store.palette.fgMuted])
          }
          .width(26)
          .height(26)
          .padding(0)
          .backgroundColor(Color.Transparent)
          .accessibilityText(
            this.collapsedNeedsYou
              ? $r('app.string.briefing_expandSection')
              : $r('app.string.briefing_collapseSection')
          )
          .onClick(() => {
            this.collapsedNeedsYou = !this.collapsedNeedsYou;
          })
        }
        .width('100%')
        .alignItems(VerticalAlign.Center)

        if (!this.collapsedNeedsYou) {
          Column({ space: OrbitTokens.SPACE_SM }) {
            ForEach(
              this.needsYouItems,
              (decision: BriefingDecisionDto) => {
                Column({ space: OrbitTokens.SPACE_SM }) {
                  OrbitAgentListItem({
                    variant: 'row',
                    title: decision.agent_title.trim().length > 0
                      ? decision.agent_title.trim()
                      : decision.agent_id,
                    status: decision.kind === 'error' ? 'failed' : 'awaiting_permission',
                    tone: (decision.kind === 'error' || this.isHighRisk(decision.risk_level))
                      ? 'danger'
                      : 'warning',
                    characterId: this.resolveCharacterId(decision.host_id, decision.agent_id),
                    avatarSeedKey: decision.agent_id,
                    provider: this.resolveLiveAgent(decision.host_id, decision.agent_id)?.provider ?? '',
                    modelLabel: this.resolveModelLabel(
                      decision.host_id,
                      this.resolveLiveAgent(decision.host_id, decision.agent_id)
                    ),
                    effortLabel: resolveAgentEffortDisplay(
                      this.resolveLiveAgent(decision.host_id, decision.agent_id)
                    ),
                    agentLabel: resolveAgentFallbackLabel(
                      this.resolveLiveAgent(decision.host_id, decision.agent_id)
                    ),
                    statusLabel: decision.kind === 'plan'
                      ? $r('app.string.briefing_planReviewBadge')
                      : (decision.kind === 'error'
                        ? $r('app.string.briefing_errorBadge')
                        : $r('app.string.overview_subagentApprovalRequired')),
                    secondaryMeta: this.hostItems.length > 1 ? decision.host_name : '',
                    titleTrailingText: formatBriefingElapsedResource(decision.updated_at_ms, Date.now()),
                    onItemClick: () => {
                      this.openWorkerSession(
                        decision.host_id,
                        decision.agent_id,
                        decision.kind === 'permission' ? 'permission_required' : '',
                        decision.request_id ?? ''
                      );
                    }
                  })

                  this.renderDecisionFooter(decision)
                }
                .width('100%')
                .padding(OrbitTokens.SPACE_MD)
                .borderRadius(OrbitTokens.RADIUS_LG)
                .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
                .border(
                  (decision.kind === 'error' || this.isHighRisk(decision.risk_level))
                    ? { width: 1, color: this.store.palette.statusDanger }
                    : { width: 1, color: this.store.palette.statusWarning }
                )
                .alignItems(HorizontalAlign.Start)
              },
              (decision: BriefingDecisionDto): string => `ny_${decision.key}`
            )
          }
          .width('100%')
        }
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
    }
  }

  @Builder
  private renderWorkerRowItem(item: PartitionedBriefingWorker) {
    OrbitAgentListItem({
      variant: 'row',
      title: item.worker.title.trim().length > 0 ? item.worker.title.trim() : item.worker.agent_id,
      status: item.status,
      characterId: this.resolveCharacterId(item.worker.host_id, item.worker.agent_id),
      avatarSeedKey: item.worker.agent_id,
      provider: this.resolveLiveAgent(item.worker.host_id, item.worker.agent_id)?.provider ?? item.worker.provider,
      modelLabel: this.resolveModelLabel(
        item.worker.host_id,
        this.resolveLiveAgent(item.worker.host_id, item.worker.agent_id)
      ),
      effortLabel: resolveAgentEffortDisplay(
        this.resolveLiveAgent(item.worker.host_id, item.worker.agent_id)
      ),
      agentLabel: resolveAgentFallbackLabel(
        this.resolveLiveAgent(item.worker.host_id, item.worker.agent_id)
      ) || formatProviderLabel(item.worker.provider),
      secondaryMeta: this.hostItems.length > 1 &&
        (item.parentHostId === null || item.worker.host_id !== item.parentHostId)
        ? item.worker.host_name
        : '',
      titleTrailingText: formatBriefingElapsedResource(item.worker.updated_at_ms, Date.now()),
      indentLevel: item.indentLevel,
      depth: item.depth,
      onItemClick: () => {
        this.openWorkerSession(item.worker.host_id, item.worker.agent_id);
      }
    })
  }

  @Builder
  private renderInProgressSection() {
    if (this.partitionedWorkers.inProgressCount > 0) {
      Column({ space: OrbitTokens.SPACE_XS }) {
        Row({ space: 6 }) {
          LoadingProgress()
            .width(12)
            .height(12)
            .color(this.store.palette.iconSecondary)
            .flexShrink(0)

          Text($r('app.string.briefing_inProgressTitle'))
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Medium))
            .maxLines(1)

          Text(`${this.partitionedWorkers.inProgressCount}`)
            .attributeModifier(orbitTextMono(this.store.palette.fgFaint))
            .maxLines(1)

          Blank()

          Button({ type: ButtonType.Circle, stateEffect: true }) {
            SymbolGlyph(
              this.collapsedInProgress ? $r('sys.symbol.chevron_right') : $r('sys.symbol.chevron_down')
            )
              .fontSize(11)
              .fontColor([this.store.palette.fgMuted])
          }
          .width(26)
          .height(26)
          .padding(0)
          .backgroundColor(Color.Transparent)
          .accessibilityText(
            this.collapsedInProgress
              ? $r('app.string.briefing_expandSection')
              : $r('app.string.briefing_collapseSection')
          )
          .onClick(() => {
            this.collapsedInProgress = !this.collapsedInProgress;
          })
        }
        .width('100%')
        .alignItems(VerticalAlign.Center)

        if (!this.collapsedInProgress) {
          Column({ space: 2 }) {
            ForEach(
              this.inProgressPage.visibleMissionWorkers,
              (item: PartitionedBriefingWorker) => {
                this.renderWorkerRowItem(item)
              },
              (item: PartitionedBriefingWorker): string => `prog_m_${item.key}`
            )

            if (this.inProgressPage.visibleOtherWorkers.length > 0) {
              if (this.inProgressPage.visibleMissionWorkers.length > 0) {
                Text($r('app.string.briefing_otherActiveLabel'))
                  .attributeModifier(orbitTextMeta(this.store.palette.fgFaint, FontWeight.Medium))
                  .padding({ left: 8, top: 6, bottom: 2 })
              }
              ForEach(
                this.inProgressPage.visibleOtherWorkers,
                (item: PartitionedBriefingWorker) => {
                  this.renderWorkerRowItem(item)
                },
                (item: PartitionedBriefingWorker): string => `prog_o_${item.key}`
              )
            }

            if (this.inProgressPage.showToggle) {
              Button(
                this.showAllInProgress
                  ? $r('app.string.common_showLess')
                  : $r('app.string.common_showMore', this.inProgressPage.hiddenCount),
                { type: ButtonType.Normal, stateEffect: true }
              )
                .attributeModifier(orbitButtonMeta(this.store.palette.fgMuted, Color.Transparent))
                .constraintSize({ minHeight: 30 })
                .onClick(() => {
                  this.showAllInProgress = !this.showAllInProgress;
                })
            }
          }
          .width('100%')
          .alignItems(HorizontalAlign.Start)
        }
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
    }
  }

  @Builder
  private renderWaitingSection() {
    if (this.waitingItems.length > 0) {
      Column({ space: OrbitTokens.SPACE_XS }) {
        Row({ space: 6 }) {
          SymbolGlyph($r('sys.symbol.clock'))
            .fontSize(12)
            .fontColor([this.store.palette.statusInfo])
            .flexShrink(0)

          Text($r('app.string.briefing_waitingTitle'))
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Medium))
            .maxLines(1)

          Text(`${this.waitingItems.length}`)
            .attributeModifier(orbitTextMono(this.store.palette.fgFaint))
            .maxLines(1)

          Blank()

          Button({ type: ButtonType.Circle, stateEffect: true }) {
            SymbolGlyph(
              this.collapsedWaiting ? $r('sys.symbol.chevron_right') : $r('sys.symbol.chevron_down')
            )
              .fontSize(11)
              .fontColor([this.store.palette.fgMuted])
          }
          .width(26)
          .height(26)
          .padding(0)
          .backgroundColor(Color.Transparent)
          .accessibilityText(
            this.collapsedWaiting
              ? $r('app.string.briefing_expandSection')
              : $r('app.string.briefing_collapseSection')
          )
          .onClick(() => {
            this.collapsedWaiting = !this.collapsedWaiting;
          })
        }
        .width('100%')
        .alignItems(VerticalAlign.Center)

        if (!this.collapsedWaiting) {
          Column({ space: 4 }) {
            ForEach(
              this.showAllWaiting
                ? this.waitingItems
                : this.waitingItems.slice(0, DEFAULT_VISIBLE_WAITING),
              (item: BriefingWaitItemDto) => {
                Row({ space: OrbitTokens.SPACE_SM }) {
                  SymbolGlyph(
                    item.kind === 'timer'
                      ? $r('sys.symbol.clock')
                      : (item.kind === 'goal' ? $r('sys.symbol.checkmark_circle') : $r('sys.symbol.bolt'))
                  )
                    .fontSize(13)
                    .fontColor([
                      item.kind === 'goal'
                        ? this.store.palette.statusDirect
                        : this.store.palette.statusInfo
                    ])
                    .flexShrink(0)

                  Column({ space: 2 }) {
                    Row({ space: 6 }) {
                      Text(item.title)
                        .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                        .maxLines(1)
                        .textOverflow({ overflow: TextOverflow.Ellipsis })
                        .layoutWeight(1)

                      if (item.due_at_ms !== null && item.due_at_ms > 0) {
                        Text(formatWaitCountdownResource(item.due_at_ms, Date.now()))
                          .attributeModifier(orbitTextMono(this.store.palette.fgPrimary, FontWeight.Medium))
                          .maxLines(1)
                          .flexShrink(0)
                      }
                    }
                    .width('100%')

                    if (item.detail.trim().length > 0) {
                      Text(item.detail.trim())
                        .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                        .maxLines(1)
                        .textOverflow({ overflow: TextOverflow.Ellipsis })
                    }
                  }
                  .alignItems(HorizontalAlign.Start)
                  .layoutWeight(1)
                }
                .width('100%')
                .constraintSize({ minHeight: 40 })
                .padding({ left: 8, right: 8, top: 6, bottom: 6 })
                .borderRadius(OrbitTokens.RADIUS_MD)
                .backgroundColor(this.store.palette.recessed)
                .alignItems(VerticalAlign.Center)
                .onClick(() => {
                  const pa = this.personalAgentRef;
                  if (pa !== null && item.host_id === pa.hostId && item.agent_id === pa.agentId) {
                    if (!this.embedded) {
                      this.onClose();
                    }
                    this.store.openAutomationsSheet();
                  } else {
                    this.openWorkerSession(item.host_id, item.agent_id);
                  }
                })
              },
              (item: BriefingWaitItemDto): string => `wait_${item.key}`
            )

            if (this.waitingItems.length > DEFAULT_VISIBLE_WAITING) {
              Button(
                this.showAllWaiting
                  ? $r('app.string.common_showLess')
                  : $r('app.string.common_showMore', this.waitingItems.length - DEFAULT_VISIBLE_WAITING),
                { type: ButtonType.Normal, stateEffect: true }
              )
                .attributeModifier(orbitButtonMeta(this.store.palette.fgMuted, Color.Transparent))
                .constraintSize({ minHeight: 30 })
                .onClick(() => {
                  this.showAllWaiting = !this.showAllWaiting;
                })
            }
          }
          .width('100%')
          .alignItems(HorizontalAlign.Start)
        }
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
    }
  }

  @Builder
  private renderCompletedSection() {
    if (this.partitionedWorkers.completedWorkers.length > 0) {
      Column({ space: OrbitTokens.SPACE_XS }) {
        Row({ space: 6 }) {
          Text($r('app.string.briefing_completedTitle'))
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Medium))
            .maxLines(1)

          Text(`${this.partitionedWorkers.completedWorkers.length}`)
            .attributeModifier(orbitTextMono(this.store.palette.fgFaint))
            .maxLines(1)

          Blank()

          if (this.partitionedWorkers.archivableCompletedWorkers.length > 0) {
            Button($r('app.string.overview_archiveDone'), { type: ButtonType.Normal, stateEffect: true })
              .attributeModifier(orbitButtonMeta(this.store.palette.fgSecondary, Color.Transparent))
              .constraintSize({ minHeight: 28 })
              .padding({ left: 8, right: 8 })
              .onClick(() => {
                this.handleArchiveDone();
              })
          }

          Button({ type: ButtonType.Circle, stateEffect: true }) {
            SymbolGlyph(
              this.collapsedCompleted ? $r('sys.symbol.chevron_right') : $r('sys.symbol.chevron_down')
            )
              .fontSize(11)
              .fontColor([this.store.palette.fgMuted])
          }
          .width(26)
          .height(26)
          .padding(0)
          .backgroundColor(Color.Transparent)
          .accessibilityText(
            this.collapsedCompleted
              ? $r('app.string.briefing_expandSection')
              : $r('app.string.briefing_collapseSection')
          )
          .onClick(() => {
            this.collapsedCompleted = !this.collapsedCompleted;
          })
        }
        .width('100%')
        .alignItems(VerticalAlign.Center)

        if (!this.collapsedCompleted) {
          Column({ space: 2 }) {
            ForEach(
              this.showAllCompleted
                ? this.partitionedWorkers.completedWorkers
                : this.partitionedWorkers.completedWorkers.slice(0, DEFAULT_VISIBLE_WORKERS),
              (item: PartitionedBriefingWorker) => {
                this.renderWorkerRowItem(item)
              },
              (item: PartitionedBriefingWorker): string => `done_${item.key}`
            )

            if (this.partitionedWorkers.completedWorkers.length > DEFAULT_VISIBLE_WORKERS) {
              Button(
                this.showAllCompleted
                  ? $r('app.string.common_showLess')
                  : $r(
                    'app.string.common_showMore',
                    this.partitionedWorkers.completedWorkers.length - DEFAULT_VISIBLE_WORKERS
                  ),
                { type: ButtonType.Normal, stateEffect: true }
              )
                .attributeModifier(orbitButtonMeta(this.store.palette.fgMuted, Color.Transparent))
                .constraintSize({ minHeight: 30 })
                .onClick(() => {
                  this.showAllCompleted = !this.showAllCompleted;
                })
            }
          }
          .width('100%')
          .alignItems(HorizontalAlign.Start)
        }
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
    }
  }

  @Builder
  private renderHandledForYouSection() {
    if (this.filteredActivity.length > 0) {
      Column({ space: OrbitTokens.SPACE_XS }) {
        Row({ space: 6 }) {
          Text($r('app.string.briefing_handledForYouTitle'))
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Medium))
            .maxLines(1)

          Text(`${this.filteredActivity.length}`)
            .attributeModifier(orbitTextMono(this.store.palette.fgFaint))
            .maxLines(1)

          Blank()

          Button({ type: ButtonType.Circle, stateEffect: true }) {
            SymbolGlyph(
              this.collapsedHandled ? $r('sys.symbol.chevron_right') : $r('sys.symbol.chevron_down')
            )
              .fontSize(11)
              .fontColor([this.store.palette.fgMuted])
          }
          .width(26)
          .height(26)
          .padding(0)
          .backgroundColor(Color.Transparent)
          .accessibilityText(
            this.collapsedHandled
              ? $r('app.string.briefing_expandSection')
              : $r('app.string.briefing_collapseSection')
          )
          .onClick(() => {
            this.collapsedHandled = !this.collapsedHandled;
          })
        }
        .width('100%')
        .alignItems(VerticalAlign.Center)

        if (!this.collapsedHandled) {
          Column({ space: 4 }) {
            ForEach(
              this.showAllActivity
                ? this.filteredActivity
                : this.filteredActivity.slice(0, DEFAULT_VISIBLE_ACTIVITY),
              (entry: PersonalAgentActivityEntryDto) => {
                Column({ space: 4 }) {
                  Row({ space: 8 }) {
                    SymbolGlyph(
                      entry.decision.toLowerCase() === 'deny'
                        ? $r('sys.symbol.xmark')
                        : $r('sys.symbol.checkmark')
                    )
                      .fontSize(12)
                      .fontColor([
                        entry.decision.toLowerCase() === 'deny'
                          ? this.store.palette.statusDanger
                          : this.store.palette.statusDirect
                      ])
                      .flexShrink(0)

                    Column({ space: 2 }) {
                      Row({ space: 6 }) {
                        Text(entry.agent_title)
                          .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                          .maxLines(1)
                          .textOverflow({ overflow: TextOverflow.Ellipsis })
                          .layoutWeight(1)

                        Text(formatBriefingElapsedResource(entry.timestamp_ms, Date.now()))
                          .attributeModifier(orbitTextMono(this.store.palette.fgFaint))
                          .maxLines(1)
                          .flexShrink(0)
                      }
                      .width('100%')

                      Text(entry.summary)
                        .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                        .maxLines(1)
                        .textOverflow({ overflow: TextOverflow.Ellipsis })

                      Row({ space: 4 }) {
                        Text(
                          entry.decision.toLowerCase() === 'deny'
                            ? $r('app.string.briefing_decisionDenied')
                            : $r('app.string.briefing_decisionAllowed')
                        )
                          .attributeModifier(
                            orbitTextMeta(
                              entry.decision.toLowerCase() === 'deny'
                                ? this.store.palette.statusDanger
                                : this.store.palette.statusDirect,
                              FontWeight.Medium
                            )
                          )

                        Text('·') // i18n-ignore
                          .attributeModifier(orbitTextMeta(this.store.palette.fgFaint))

                        Text(this.actorLabelResource(entry.actor))
                          .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                          .maxLines(1)
                      }
                    }
                    .alignItems(HorizontalAlign.Start)
                    .layoutWeight(1)

                    if (
                      entry.agent_id !== undefined &&
                      entry.agent_id !== null &&
                      entry.agent_id.length > 0
                    ) {
                      Button({ type: ButtonType.Circle, stateEffect: true }) {
                        SymbolGlyph($r('sys.symbol.chevron_right'))
                          .fontSize(12)
                          .fontColor([this.store.palette.fgMuted])
                      }
                      .width(28)
                      .height(28)
                      .padding(0)
                      .backgroundColor(Color.Transparent)
                      .accessibilityText($r('app.string.briefing_openSession'))
                      .onClick(() => {
                        const targetHost = (entry.host_id !== undefined && entry.host_id !== null && entry.host_id.length > 0)
                          ? entry.host_id
                          : (this.personalAgentRef?.hostId ?? this.store.selectedHostId);
                        if (entry.agent_id !== undefined && entry.agent_id !== null) {
                          this.openWorkerSession(targetHost, entry.agent_id);
                        }
                      })
                    }
                  }
                  .width('100%')
                  .alignItems(VerticalAlign.Top)
                  .onClick(() => {
                    if (entry.reason.trim().length > 0) {
                      this.toggleActivityExpanded(`${entry.seq}:${entry.timestamp_ms}`);
                    }
                  })

                  if (
                    entry.reason.trim().length > 0 &&
                    this.isActivityExpanded(`${entry.seq}:${entry.timestamp_ms}`)
                  ) {
                    Text(entry.reason.trim())
                      .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                      .padding({ left: 20, top: 2 })
                      .width('100%')
                  }
                }
                .width('100%')
                .padding({ left: 8, right: 8, top: 6, bottom: 6 })
                .borderRadius(OrbitTokens.RADIUS_MD)
                .backgroundColor(
                  this.isActivityExpanded(`${entry.seq}:${entry.timestamp_ms}`)
                    ? this.store.palette.recessed
                    : Color.Transparent
                )
              },
              (entry: PersonalAgentActivityEntryDto): string => `act_${entry.seq}_${entry.timestamp_ms}`
            )

            if (this.filteredActivity.length > DEFAULT_VISIBLE_ACTIVITY) {
              Button(
                this.showAllActivity
                  ? $r('app.string.common_showLess')
                  : $r('app.string.common_showMore', this.filteredActivity.length - DEFAULT_VISIBLE_ACTIVITY),
                { type: ButtonType.Normal, stateEffect: true }
              )
                .attributeModifier(orbitButtonMeta(this.store.palette.fgMuted, Color.Transparent))
                .constraintSize({ minHeight: 30 })
                .onClick(() => {
                  this.showAllActivity = !this.showAllActivity;
                })
            }
          }
          .width('100%')
          .alignItems(HorizontalAlign.Start)
        }
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
    }
  }

  @Builder
  private renderHostsSection() {
    if (this.hostItems.length > 1) {
      Column({ space: OrbitTokens.SPACE_XS }) {
        Row({ space: 6 }) {
          Text($r('app.string.briefing_hostsTitle'))
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Medium))
            .maxLines(1)

          Blank()

          Text(
            $r(
              'app.string.briefing_hostsOnlineSummary',
              this.briefingData?.counts.hosts_online ?? 0,
              this.briefingData?.counts.hosts_total ?? this.hostItems.length
            )
          )
            .attributeModifier(orbitTextMono(this.store.palette.fgSecondary))
            .maxLines(1)

          Button({ type: ButtonType.Circle, stateEffect: true }) {
            SymbolGlyph(
              this.collapsedHosts ? $r('sys.symbol.chevron_right') : $r('sys.symbol.chevron_down')
            )
              .fontSize(11)
              .fontColor([this.store.palette.fgMuted])
          }
          .width(26)
          .height(26)
          .padding(0)
          .backgroundColor(Color.Transparent)
          .accessibilityText(
            this.collapsedHosts
              ? $r('app.string.briefing_expandSection')
              : $r('app.string.briefing_collapseSection')
          )
          .onClick(() => {
            this.collapsedHosts = !this.collapsedHosts;
          })
        }
        .width('100%')
        .alignItems(VerticalAlign.Center)

        if (!this.collapsedHosts) {
          Flex({ wrap: FlexWrap.Wrap, space: { main: LengthMetrics.vp(6), cross: LengthMetrics.vp(6) } }) {
            ForEach(
              this.hostItems,
              (host: BriefingHostDto) => {
                Row({ space: 6 }) {
                  Row()
                    .width(6)
                    .height(6)
                    .borderRadius(3)
                    .backgroundColor(
                      host.online ? this.store.palette.statusDirect : this.store.palette.fgMuted
                    )
                    .flexShrink(0)

                  Text(host.name)
                    .attributeModifier(
                      orbitTextMeta(
                        host.is_personal_agent_host
                          ? this.store.palette.fgPrimary
                          : this.store.palette.fgSecondary,
                        host.is_personal_agent_host ? FontWeight.Medium : FontWeight.Normal
                      )
                    )
                    .maxLines(1)
                    .textOverflow({ overflow: TextOverflow.Ellipsis })
                    .constraintSize({ maxWidth: 120 })

                  if (host.is_personal_agent_host) {
                    Image($r('app.media.orbit_agent_bot'))
                      .width(11)
                      .height(11)
                      .objectFit(ImageFit.Contain)
                      .fillColor($r('app.color.icon_primary'))
                      .draggable(false)
                      .flexShrink(0)
                  }
                }
                .padding({ left: 8, right: 8, top: 4, bottom: 4 })
                .borderRadius(OrbitTokens.RADIUS_XS)
                .backgroundColor(
                  host.is_personal_agent_host
                    ? this.store.palette.recessed
                    : this.store.palette.listCard
                )
                .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
                .alignItems(VerticalAlign.Center)
                .onClick(() => {
                  this.store.selectHost(host.host_id);
                })
              },
              (host: BriefingHostDto): string => `bhost_${host.host_id}`
            )
          }
          .width('100%')
        }
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
    }
  }

  private hasAnyWorkSection(): boolean {
    return (
      this.needsYouItems.length > 0 ||
      this.partitionedWorkers.inProgressCount > 0 ||
      this.waitingItems.length > 0 ||
      this.partitionedWorkers.completedWorkers.length > 0 ||
      this.filteredActivity.length > 0
    );
  }

  build() {
    Column({ space: 0 }) {
      // Header bar
      Row({ space: OrbitTokens.SPACE_SM }) {
        Image($r('app.media.orbit_agent_bot'))
          .width(15)
          .height(15)
          .objectFit(ImageFit.Contain)
          .fillColor($r('app.color.icon_primary'))
          .draggable(false)
          .flexShrink(0)

        Text($r('app.string.briefing_title'))
          .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold))
          .maxLines(1)
          .layoutWeight(1)

        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.gearshape'))
            .fontSize(15)
            .fontColor([this.store.palette.iconSecondary])
        }
        .width(32)
        .height(32)
        .padding(0)
        .backgroundColor(Color.Transparent)
        .accessibilityText($r('app.string.briefing_openSettings'))
        .onClick(() => {
          if (!this.embedded) {
            this.onClose();
          }
          this.store.openRouterMeshSheet();
        })

        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.xmark'))
            .fontSize(14)
            .fontColor([this.store.palette.iconPrimary])
        }
        .width(32)
        .height(32)
        .padding(0)
        .backgroundColor(this.store.palette.subtleSurface)
        .accessibilityText($r('app.string.briefing_hideBriefing'))
        .onClick(() => {
          this.onClose();
        })
      }
      .width('100%')
      .constraintSize({ minHeight: 48 })
      .padding({
        left: OrbitTokens.SPACE_MD,
        right: OrbitTokens.SPACE_MD,
        top: OrbitTokens.SPACE_SM,
        bottom: OrbitTokens.SPACE_SM
      })
      .alignItems(VerticalAlign.Center)

      Divider()
        .strokeWidth(OrbitTokens.HAIRLINE)
        .color(this.store.palette.hairline)
        .width('100%')

      // Six-section scrollable briefing body
      Refresh({ refreshing: $$this.isRefreshing }) {
        Scroll() {
          Column({ space: OrbitTokens.SPACE_MD }) {
            this.renderAwayBanner()
            this.renderNeedsYouSection()
            this.renderInProgressSection()
            this.renderWaitingSection()
            this.renderCompletedSection()
            this.renderHandledForYouSection()

            if (!this.hasAnyWorkSection() && this.hostItems.length <= 1) {
              Text($r('app.string.briefing_allQuiet'))
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                .width('100%')
                .textAlign(TextAlign.Center)
                .padding({ top: OrbitTokens.SPACE_XL, bottom: OrbitTokens.SPACE_XL })
            }

            this.renderHostsSection()
          }
          .width('100%')
          .padding({
            left: OrbitTokens.SPACE_MD,
            right: OrbitTokens.SPACE_MD,
            top: OrbitTokens.SPACE_MD,
            bottom: this.embedded
              ? OrbitTokens.SPACE_XL
              : Math.max(OrbitTokens.SPACE_XL, this.store.safeBottomVp)
          })
          .alignItems(HorizontalAlign.Start)
        }
        .width('100%')
        .height('100%')
        .scrollBar(BarState.Auto)
        .edgeEffect(EdgeEffect.Spring)
      }
      .onRefreshing(() => {
        this.handleRefresh();
      })
      .width('100%')
      .layoutWeight(1)
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.canvas)
    .accessibilityText($r('app.string.briefing_ariaLabel'))
  }
}
