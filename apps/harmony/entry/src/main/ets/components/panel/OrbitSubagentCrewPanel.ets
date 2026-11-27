import {
  SessionSubagentEntryDto,
  isSubagentActiveStatus,
  isSubagentSettledStatus,
  resolveSubagentActualHostId,
  resolveSubagentDirectParentAgentId,
  resolveSubagentDirectParentHostId
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { orbitRuntime } from '../../model/OrbitRuntime';
import {
  OrbitSidePanelTab,
  encodeSidePanelKeyTuple
} from '../../model/stores/OrbitSidePanelStore';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitInputSubhead,
  orbitTextBody,
  orbitTextMeta,
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import { OrbitTheme } from '../../theme/OrbitTheme';
import { dismissHarmonyKeyboard } from '../common/OrbitCommonControls';

function isRunningSubagentStatus(status: string): boolean {
  const s = status.trim().toLowerCase();
  return s === 'running' || s === 'working' || s === 'streaming' || s === 'initializing';
}

function subagentStatusLabel(status: string): string {
  const s = status.trim().toLowerCase();
  if (s === 'awaiting_permission') {
    return 'Needs approval';
  }
  if (isRunningSubagentStatus(s)) {
    return 'Working';
  }
  if (s === 'denied') {
    return 'Denied';
  }
  if (s === 'failed' || s === 'error') {
    return 'Failed';
  }
  if (s === 'completed' || s === 'done' || s === 'succeeded') {
    return 'Done';
  }
  if (s === 'canceled' || s === 'cancelled') {
    return 'Canceled';
  }
  return status.length > 0 ? status : 'Idle';
}

/**
 * Isolated Crew row subcomponent (structured for drop-in alignment with `OrbitAgentListItem` once PR-2 merges).
 */
@ComponentV2
struct OrbitCrewMemberRow {
  @Param @Require sub: SessionSubagentEntryDto;
  @Param @Require rootHostId: string;
  @Param isSteering: boolean = false;
  @Param steerDraft: string = '';
  @Event onOpen: () => void = () => {};
  @Event onToggleSteer: () => void = () => {};
  @Event onSteerDraftChange: (val: string) => void = (_val: string) => {};
  @Event onSubmitSteer: () => void = () => {};
  @Event onArchive: () => void = () => {};

  private readonly store: OrbitHarmonyStore = orbitRuntime();

  private statusColor(status: string): ResourceColor {
    const s = status.trim().toLowerCase();
    if (s === 'awaiting_permission') {
      return this.store.palette.statusWarning;
    }
    if (isRunningSubagentStatus(s)) {
      return this.store.palette.accentPrimary;
    }
    if (s === 'failed' || s === 'error' || s === 'denied') {
      return this.store.palette.statusDanger;
    }
    if (s === 'completed' || s === 'done' || s === 'succeeded') {
      return this.store.palette.statusDirect;
    }
    return this.store.palette.fgMuted;
  }

  private get remoteHostTag(): string {
    const actualHost = resolveSubagentActualHostId(this.sub, this.rootHostId);
    if (actualHost.length > 0 && actualHost !== this.rootHostId) {
      return (this.sub.host_name !== undefined && this.sub.host_name !== null && this.sub.host_name.length > 0)
        ? this.sub.host_name
        : actualHost;
    }
    return '';
  }

  build() {
    Column({ space: OrbitTokens.SPACE_SM }) {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Row()
          .width(8)
          .height(8)
          .borderRadius(4)
          .backgroundColor(this.statusColor(this.sub.status))

        Column({ space: 2 }) {
          Row({ space: 6 }) {
            Text(this.sub.role.trim().length > 0 ? this.sub.role.trim() : 'Subagent')
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
              .layoutWeight(1)
            if (this.remoteHostTag.length > 0) {
              Text(this.remoteHostTag)
                .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                .padding({ left: 6, right: 6, top: 1, bottom: 1 })
                .backgroundColor(this.store.palette.subtleSurface)
                .borderRadius(OrbitTokens.RADIUS_SM)
                .maxLines(1)
            }
          }
          .width('100%')
          .alignItems(VerticalAlign.Center)

          Text(
            this.sub.prompt_preview.trim().length > 0
              ? `${subagentStatusLabel(this.sub.status)} · ${this.sub.prompt_preview.trim()}`
              : subagentStatusLabel(this.sub.status)
          )
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)
        .onClick(() => {
          this.onOpen();
        })

        if (this.sub.child_agent_id !== null && this.sub.child_agent_id.length > 0) {
          Button('Steer', { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(
              orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface)
            )
            .height(28)
            .padding({ left: 10, right: 10 })
            .onClick(() => {
              this.onToggleSteer();
            })
        }

        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.archivebox'))
            .fontSize(14)
            .fontColor([this.store.palette.iconSecondary])
        }
        .width(30)
        .height(30)
        .padding(0)
        .backgroundColor(this.store.palette.subtleSurface)
        .accessibilityText(`Archive ${this.sub.role || 'subagent'}`)
        .onClick(() => {
          this.onArchive();
        })
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)

      if (this.isSteering && this.sub.child_agent_id !== null && this.sub.child_agent_id.length > 0) {
        Row({ space: OrbitTokens.SPACE_XS }) {
          TextInput({ placeholder: 'Send guidance to subagent…', text: this.steerDraft })
            .attributeModifier(
              orbitInputSubhead(
                this.store.palette.fgPrimary,
                this.store.palette.fgMuted,
                this.store.palette.recessed
              )
            )
            .height(36)
            .borderRadius(OrbitTokens.RADIUS_MD)
            .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
            .layoutWeight(1)
            .onFocus(() => {
              this.store.shellStore.setTextInputFocused(true);
            })
            .onBlur(() => {
              this.store.shellStore.setTextInputFocused(false);
            })
            .onChange((val: string) => {
              this.onSteerDraftChange(val);
            })
            .onSubmit(() => {
              this.onSubmitSteer();
            })

          Button('Send', { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(orbitButtonMeta(this.store.palette.onAccent, this.store.palette.accentPrimaryFill))
            .height(36)
            .padding({ left: 12, right: 12 })
            .enabled(this.steerDraft.trim().length > 0)
            .onClick(() => {
              this.onSubmitSteer();
            })
        }
        .width('100%')
      }
    }
    .width('100%')
    .padding(OrbitTokens.SPACE_MD)
    .borderRadius(OrbitTokens.RADIUS_MD)
    .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
  }
}

/**
 * Right Side Panel Subagent Crew manager.
 * Strictly isolated to `tab.parentHostId`/`tab.hostId` and `tab.parentAgentId`/`tab.agentId`,
 * preserving every descendant's actual `host_id` for inspection, Steer, and fail-closed Archive.
 */
@ComponentV2
export struct OrbitSubagentCrewPanel {
  @Param @Require tab: OrbitSidePanelTab;
  @Param scopeKey: string = '';

  @Local private steeringSubagentKey: string = '';
  @Local private steerDraft: string = '';

  private lastTargetKey: string = '';
  private readonly store: OrbitHarmonyStore = orbitRuntime();

  aboutToAppear(): void {
    this.lastTargetKey = this.computeTargetKey();
    if (this.boundHostId.length > 0) {
      this.store.ensureScopedHostDetailAsync(this.boundHostId);
    }
  }

  @Monitor('tab', 'scopeKey')
  onTargetChanged(_mon: IMonitor): void {
    const nextKey = this.computeTargetKey();
    if (nextKey !== this.lastTargetKey) {
      this.lastTargetKey = nextKey;
      this.steeringSubagentKey = '';
      this.steerDraft = '';
      if (this.boundHostId.length > 0) {
        this.store.ensureScopedHostDetailAsync(this.boundHostId);
      }
    }
  }

  private computeTargetKey(): string {
    return `${this.scopeKey}|${this.tab.tabId}|${this.boundHostId}|${this.boundRootAgentId}`;
  }

  @Computed
  get boundHostId(): string {
    if (this.tab.parentHostId !== null && this.tab.parentHostId.length > 0) {
      return this.tab.parentHostId;
    }
    return this.tab.hostId;
  }

  @Computed
  get boundRootAgentId(): string {
    if (this.tab.parentAgentId !== undefined && this.tab.parentAgentId !== null && this.tab.parentAgentId.length > 0) {
      return this.tab.parentAgentId;
    }
    return this.tab.agentId !== null ? this.tab.agentId : '';
  }

  @Computed
  get subagents(): SessionSubagentEntryDto[] {
    const _rev =
      this.store.stageRevision +
      this.store.panelDataRevision +
      this.store.fleetStore.hostDetailRevision +
      this.store.archivedSubagentKeys.length;
    if (_rev < 0 || this.boundHostId.length === 0 || this.boundRootAgentId.length === 0) {
      return [];
    }
    return this.store.getScopedSubagentsForSession(this.boundHostId, this.boundRootAgentId);
  }

  @Computed
  get activeCount(): number {
    let n = 0;
    const list = this.subagents;
    for (let i = 0; i < list.length; i++) {
      if (isSubagentActiveStatus(list[i].status)) {
        n += 1;
      }
    }
    return n;
  }

  @Computed
  get settledCount(): number {
    let n = 0;
    const list = this.subagents;
    for (let i = 0; i < list.length; i++) {
      if (isSubagentSettledStatus(list[i].status)) {
        n += 1;
      }
    }
    return n;
  }

  private subagentKey(sub: SessionSubagentEntryDto): string {
    const actualHost = resolveSubagentActualHostId(sub, this.boundHostId);
    if (sub.child_agent_id !== null && sub.child_agent_id.trim().length > 0) {
      return encodeSidePanelKeyTuple(['crew_managed', actualHost, sub.child_agent_id.trim()]);
    }
    const directParentHost = resolveSubagentDirectParentHostId(sub, this.boundHostId);
    const directParentAgent = resolveSubagentDirectParentAgentId(sub, this.boundRootAgentId);
    const provId = (
      sub.provider_subagent_id !== undefined &&
      sub.provider_subagent_id !== null &&
      sub.provider_subagent_id.trim().length > 0
    )
      ? sub.provider_subagent_id.trim()
      : sub.id.trim();
    return encodeSidePanelKeyTuple(['crew_provider', directParentHost, directParentAgent, provId]);
  }

  private openSubagentTab(sub: SessionSubagentEntryDto): void {
    this.store.openSubagentEntryInRootScope(
      this.boundHostId,
      this.boundRootAgentId,
      sub,
      this.tab.cwd
    );
  }

  private submitSteer(sub: SessionSubagentEntryDto): void {
    const clean = this.steerDraft.trim();
    if (clean.length === 0 || sub.child_agent_id === null || sub.child_agent_id.length === 0) {
      return;
    }
    dismissHarmonyKeyboard(this.getUIContext());
    const childHostId = resolveSubagentActualHostId(sub, this.boundHostId);
    const ok = this.store.steerScopedAgent(childHostId, sub.child_agent_id, clean);
    if (ok) {
      this.steerDraft = '';
      this.steeringSubagentKey = '';
    }
  }

  build() {
    Column({ space: OrbitTokens.SPACE_SM }) {
      // Header bar with counts + Archive done batch action
      Row({ space: OrbitTokens.SPACE_SM }) {
        Column({ space: 2 }) {
          Text('Subagent Crew')
            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold))
            .maxLines(1)
          Text(`${this.activeCount} active · ${this.subagents.length} total`)
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
            .maxLines(1)
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        if (this.settledCount > 0) {
          Button('Archive done', { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(
              orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface)
            )
            .height(30)
            .padding({ left: 10, right: 10 })
            .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
            .accessibilityText(`Archive ${this.settledCount} finished subagents`)
            .onClick(() => {
              this.store.archiveSettledSubagentsForSession(
                this.boundHostId,
                this.boundRootAgentId,
                this.subagents
              );
            })
        }
      }
      .width('100%')
      .padding({
        left: OrbitTokens.SPACE_MD,
        right: OrbitTokens.SPACE_MD,
        top: OrbitTokens.SPACE_SM,
        bottom: OrbitTokens.SPACE_XS
      })
      .alignItems(VerticalAlign.Center)

      if (this.subagents.length === 0) {
        Column({ space: OrbitTokens.SPACE_SM }) {
          Blank()
          SymbolGlyph($r('sys.symbol.person_2'))
            .fontSize(26)
            .fontColor([this.store.palette.fgMuted])
          Text('No active subagents in this session')
            .attributeModifier(orbitTextBody(this.store.palette.fgSecondary))
            .textAlign(TextAlign.Center)
          Blank()
        }
        .width('100%')
        .layoutWeight(1)
        .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
        .alignItems(HorizontalAlign.Center)
      } else {
        List({ space: OrbitTokens.SPACE_SM }) {
          ForEach(
            this.subagents,
            (sub: SessionSubagentEntryDto) => {
              ListItem() {
                OrbitCrewMemberRow({
                  sub: sub,
                  rootHostId: this.boundHostId,
                  isSteering: this.steeringSubagentKey === this.subagentKey(sub),
                  steerDraft: this.steerDraft,
                  onOpen: () => {
                    this.openSubagentTab(sub);
                  },
                  onToggleSteer: () => {
                    const key = this.subagentKey(sub);
                    this.steeringSubagentKey = this.steeringSubagentKey === key ? '' : key;
                    this.steerDraft = '';
                  },
                  onSteerDraftChange: (val: string) => {
                    this.steerDraft = val;
                  },
                  onSubmitSteer: () => {
                    this.submitSteer(sub);
                  },
                  onArchive: () => {
                    this.store.archiveScopedSubagent(this.boundHostId, this.boundRootAgentId, sub);
                  }
                })
              }
            },
            (sub: SessionSubagentEntryDto): string => `crew_row_${this.subagentKey(sub)}_${sub.status}`
          )
        }
        .width('100%')
        .layoutWeight(1)
        .padding({
          left: OrbitTokens.SPACE_MD,
          right: OrbitTokens.SPACE_MD,
          bottom: Math.max(OrbitTokens.SPACE_MD, this.store.safeBottomVp)
        })
        .scrollBar(BarState.Auto)
        .edgeEffect(EdgeEffect.Spring)
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.canvas)
  }
}
