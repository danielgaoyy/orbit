import {
  BriefingDecisionDto,
  PersonalAgentBriefingDto,
  TimelineItemDto,
  orbitTextHashSig
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { orbitRuntime } from '../../model/OrbitRuntime';
import { OrbitFleetStore } from '../../model/stores/OrbitFleetStore';
import {
  OrbitSidePanelTab,
  encodeSidePanelKeyTuple
} from '../../model/stores/OrbitSidePanelStore';
import { OrbitStageSession } from '../../model/stores/OrbitStageStore';
import {
  OrbitLiveSegmentModel,
  OrbitStageTurnModel,
  OrbitTimelineItemModel
} from '../../model/viewmodels/OrbitStageModels';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitInputSubhead,
  orbitTextBody,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import { OrbitTheme } from '../../theme/OrbitTheme';
import { dismissHarmonyKeyboard } from '../common/OrbitCommonControls';
import { RichMarkdownView } from '../stage/RichMarkdownView';

function summarizeScopedItem(item: TimelineItemDto): string {
  if ((item.text ?? '').trim().length > 0) {
    return (item.text ?? '').trim();
  }
  if ((item.summary_title ?? '').trim().length > 0) {
    return (item.summary_title ?? '').trim();
  }
  if (item.detail !== undefined) {
    if ((item.detail.command ?? '').trim().length > 0) {
      return `$ ${(item.detail.command ?? '').trim()}`;
    }
    if ((item.detail.path ?? '').trim().length > 0) {
      return (item.detail.path ?? '').trim();
    }
    if ((item.detail.tool_name ?? '').trim().length > 0) {
      return (item.detail.tool_name ?? '').trim();
    }
  }
  return item.item_type;
}

function summarizeTimelineModel(item: OrbitTimelineItemModel): string {
  if (item.rawText.trim().length > 0) {
    return item.rawText.trim();
  }
  if (item.title.trim().length > 0) {
    return item.title.trim();
  }
  if (item.detail !== null) {
    if ((item.detail.command ?? '').trim().length > 0) {
      return `$ ${(item.detail.command ?? '').trim()}`;
    }
    if ((item.detail.path ?? '').trim().length > 0) {
      return (item.detail.path ?? '').trim();
    }
    if ((item.detail.tool_name ?? '').trim().length > 0) {
      return (item.detail.tool_name ?? '').trim();
    }
  }
  return item.itemType;
}

function turnLiveSegmentsContainKey(turn: OrbitStageTurnModel, itemKey: string): boolean {
  for (let i = 0; i < turn.liveSegments.length; i++) {
    const seg = turn.liveSegments[i];
    if (seg.item !== null && seg.item.itemKey === itemKey) {
      return true;
    }
    for (let j = 0; j < seg.items.length; j++) {
      if (seg.items[j].itemKey === itemKey) {
        return true;
      }
    }
  }
  return false;
}

function countSessionRenderedItems(session: OrbitStageSession): number {
  let count = 0;
  for (let i = 0; i < session.turns.length; i++) {
    const turn = session.turns[i];
    if (turn.userPrompt !== null) {
      count += 1;
    }
    if (turn.agentMessage !== null) {
      count += 1;
    }
    count += turn.foldedSteps.length;
    for (let j = 0; j < turn.liveSegments.length; j++) {
      const seg = turn.liveSegments[j];
      if (seg.item !== null) {
        count += 1;
      }
      count += seg.items.length;
    }
    if (turn.terminalReply !== null && !turnLiveSegmentsContainKey(turn, turn.terminalReply.itemKey)) {
      count += 1;
    }
    count += turn.notices.length;
  }
  return count;
}

/**
 * Fine-grained incremental card consuming `@ObservedV2 OrbitTimelineItemModel` directly so ArkUI V2
 * updates mutated text/blocks in-place without rescanning or re-allocating the entire timeline window (P1-4).
 */
@ComponentV2
struct OrbitScopedTimelineModelCard {
  @Param @Require item: OrbitTimelineItemModel;
  @Param renderMarkdown: boolean = true;

  @Local private store: OrbitHarmonyStore = orbitRuntime();

  build() {
    Column({ space: OrbitTokens.SPACE_XS }) {
      Row({ space: OrbitTokens.SPACE_XS }) {
        Text(this.item.itemType.replace(/_/g, ' '))
          .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Medium))
        Blank()
        Text(`#${this.item.seq}`)
          .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
      }
      .width('100%')

      if (this.renderMarkdown && this.item.itemType === 'assistant_message') {
        RichMarkdownView({
          blocks: this.item.rawBlocks,
          fallbackText: this.item.rawText
        })
      } else {
        Text(summarizeTimelineModel(this.item))
          .attributeModifier(
            this.item.itemType === 'tool_call'
              ? orbitTextMono(this.store.palette.fgPrimary)
              : orbitTextBody(this.store.palette.fgPrimary)
          )
          .width('100%')
          .copyOption(CopyOptions.LocalDevice)
      }
    }
    .width('100%')
    .padding(OrbitTokens.SPACE_MD)
    .borderRadius(OrbitTokens.RADIUS_MD)
    .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
    .alignItems(HorizontalAlign.Start)
  }
}

/**
 * Incremental turn-level renderer for managed subagent & sibling peer timelines (P1-4).
 * Handles userPrompt, agentMessage, foldedSteps, liveSegments (both single-item `seg.item`
 * and array `seg.items`), terminalReply (deduplicated against liveSegments), and notices in order.
 */
@ComponentV2
struct OrbitScopedStageTurnList {
  @Param @Require session: OrbitStageSession;
  @Param renderMarkdown: boolean = true;

  build() {
    List({ space: OrbitTokens.SPACE_SM }) {
      ForEach(
        this.session.turns,
        (turn: OrbitStageTurnModel) => {
          ListItem() {
            Column({ space: OrbitTokens.SPACE_SM }) {
              if (turn.userPrompt !== null) {
                OrbitScopedTimelineModelCard({
                  item: turn.userPrompt,
                  renderMarkdown: this.renderMarkdown
                })
              }
              if (turn.agentMessage !== null) {
                OrbitScopedTimelineModelCard({
                  item: turn.agentMessage,
                  renderMarkdown: this.renderMarkdown
                })
              }
              ForEach(
                turn.foldedSteps,
                (stepItem: OrbitTimelineItemModel) => {
                  OrbitScopedTimelineModelCard({
                    item: stepItem,
                    renderMarkdown: this.renderMarkdown
                  })
                },
                (stepItem: OrbitTimelineItemModel): string => `fold_${turn.turnKey}_${stepItem.itemKey}`
              )
              ForEach(
                turn.liveSegments,
                (seg: OrbitLiveSegmentModel) => {
                  Column({ space: OrbitTokens.SPACE_SM }) {
                    if (seg.item !== null) {
                      OrbitScopedTimelineModelCard({
                        item: seg.item,
                        renderMarkdown: this.renderMarkdown
                      })
                    }
                    ForEach(
                      seg.items,
                      (segItem: OrbitTimelineItemModel) => {
                        OrbitScopedTimelineModelCard({
                          item: segItem,
                          renderMarkdown: this.renderMarkdown
                        })
                      },
                      (segItem: OrbitTimelineItemModel): string => `segi_${seg.segKey}_${segItem.itemKey}`
                    )
                  }
                  .width('100%')
                },
                (seg: OrbitLiveSegmentModel): string => `seg_${turn.turnKey}_${seg.segKey}`
              )
              if (
                turn.terminalReply !== null &&
                !turnLiveSegmentsContainKey(turn, turn.terminalReply.itemKey)
              ) {
                OrbitScopedTimelineModelCard({
                  item: turn.terminalReply,
                  renderMarkdown: this.renderMarkdown
                })
              }
              ForEach(
                turn.notices,
                (noticeItem: OrbitTimelineItemModel) => {
                  OrbitScopedTimelineModelCard({
                    item: noticeItem,
                    renderMarkdown: this.renderMarkdown
                  })
                },
                (noticeItem: OrbitTimelineItemModel): string => `ntc_${turn.turnKey}_${noticeItem.itemKey}`
              )
            }
            .width('100%')
          }
        },
        (turn: OrbitStageTurnModel): string => `turn_${turn.turnKey}`
      )
    }
    .width('100%')
    .layoutWeight(1)
    .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
    .scrollBar(BarState.Auto)
    .edgeEffect(EdgeEffect.Spring)
  }
}

/**
 * Individual Subagent inspector tab (`kind === 'subagent'` and not a Crew overview tab).
 * Strictly isolated by target identity (`scopeKey`, `tabId`, `hostId`, `childAgentId`, `providerSubagentId`):
 * - Managed child subagents subscribe ONCE per target identity and observe `boundSession.commitRevision`
 *   directly (zero duplicate in-flight fetches, zero stale copies, zero re-opening on unrelated revisions).
 * - Provider-native subagents subscribe ONCE using direct parent `(directParentHostId, directParentAgentId)`
 *   and parse the real `Vec<ProviderSubagentEvent>` JSON array.
 */
@ComponentV2
export struct OrbitScopedSubagentTabView {
  @Param @Require tab: OrbitSidePanelTab;
  @Param scopeKey: string = '';

  @Local private boundSession: OrbitStageSession | null = null;
  @Local private providerItems: TimelineItemDto[] = [];
  @Local private steerText: string = '';

  private loadSeq: number = 0;
  private lastTargetKey: string = '';
  @Local private store: OrbitHarmonyStore = orbitRuntime();

  aboutToAppear(): void {
    this.rebindToCurrentTarget();
  }

  @Monitor('tab', 'scopeKey')
  onTargetChanged(_mon: IMonitor): void {
    const nextKey = this.computeTargetKey();
    if (nextKey !== this.lastTargetKey) {
      this.rebindToCurrentTarget();
    }
  }

  @Monitor('store.providerSubagentRevision')
  onProviderSubagentRevisionChanged(_mon: IMonitor): void {
    if (this.effectiveChildAgentId === null && this.effectiveProviderSubagentId !== null) {
      this.reloadProviderItems(false);
    }
  }

  private get effectiveChildAgentId(): string | null {
    if (this.tab.childAgentId !== undefined && this.tab.childAgentId !== null && this.tab.childAgentId.trim().length > 0) {
      return this.tab.childAgentId.trim();
    }
    if (
      (this.tab.providerSubagentId === undefined || this.tab.providerSubagentId === null || this.tab.providerSubagentId.trim().length === 0) &&
      this.tab.agentId !== null &&
      this.tab.agentId.trim().length > 0
    ) {
      return this.tab.agentId.trim();
    }
    return null;
  }

  private get effectiveProviderSubagentId(): string | null {
    if (
      this.tab.providerSubagentId !== undefined &&
      this.tab.providerSubagentId !== null &&
      this.tab.providerSubagentId.trim().length > 0
    ) {
      return this.tab.providerSubagentId.trim();
    }
    return null;
  }

  private get directParentHostId(): string {
    if (
      this.tab.directParentHostId !== undefined &&
      this.tab.directParentHostId !== null &&
      this.tab.directParentHostId.trim().length > 0
    ) {
      return this.tab.directParentHostId.trim();
    }
    if (this.tab.parentHostId !== null && this.tab.parentHostId.trim().length > 0) {
      return this.tab.parentHostId.trim();
    }
    return this.tab.hostId.trim();
  }

  private get directParentAgentId(): string {
    if (
      this.tab.directParentAgentId !== undefined &&
      this.tab.directParentAgentId !== null &&
      this.tab.directParentAgentId.trim().length > 0
    ) {
      return this.tab.directParentAgentId.trim();
    }
    if (this.tab.parentAgentId !== undefined && this.tab.parentAgentId !== null && this.tab.parentAgentId.trim().length > 0) {
      return this.tab.parentAgentId.trim();
    }
    return '';
  }

  private computeTargetKey(): string {
    return encodeSidePanelKeyTuple([
      'subagent',
      this.scopeKey,
      this.tab.tabId,
      this.tab.hostId,
      this.effectiveChildAgentId ?? '',
      this.effectiveProviderSubagentId ?? '',
      this.directParentHostId,
      this.directParentAgentId
    ]);
  }

  private rebindToCurrentTarget(): void {
    // Always increment generation FIRST and clear transient input/content before any early return (P1-1).
    this.loadSeq += 1;
    this.lastTargetKey = this.computeTargetKey();
    this.steerText = '';
    this.boundSession = null;
    this.providerItems = [];

    const childId = this.effectiveChildAgentId;
    if (childId !== null && this.tab.hostId.trim().length > 0) {
      this.boundSession = this.store.bindScopedManagedSession(this.tab.hostId.trim(), childId);
      return;
    }
    if (this.effectiveProviderSubagentId !== null) {
      this.reloadProviderItems(true);
    }
  }

  private reloadProviderItems(subscribe: boolean): void {
    const provId = this.effectiveProviderSubagentId;
    if (provId === null || this.directParentHostId.length === 0 || this.directParentAgentId.length === 0) {
      this.providerItems = [];
      return;
    }
    this.providerItems = this.store.loadScopedProviderSubagentTimeline(
      this.directParentHostId,
      this.directParentAgentId,
      provId,
      subscribe
    );
  }

  @Computed
  get displayedEventCount(): number {
    if (this.boundSession !== null) {
      const _commit = this.boundSession.commitRevision;
      if (_commit < 0) {
        return 0;
      }
      return countSessionRenderedItems(this.boundSession);
    }
    return this.providerItems.length;
  }

  private manualRefresh(): void {
    if (this.boundSession !== null) {
      this.store.stageStore.fetchIncremental(this.boundSession, 24).catch((_err: Object) => {
        // Handled by OrbitStageStore
      });
    } else if (this.effectiveProviderSubagentId !== null) {
      this.reloadProviderItems(true);
    }
  }

  private submitSteer(): void {
    const childId = this.effectiveChildAgentId;
    const targetHost = this.tab.hostId.trim();
    const clean = this.steerText.trim();
    if (childId === null || clean.length === 0 || targetHost.length === 0) {
      return;
    }
    dismissHarmonyKeyboard(this.getUIContext());
    if (this.store.steerScopedAgent(targetHost, childId, clean)) {
      this.steerText = '';
    }
  }

  build() {
    Column({ space: OrbitTokens.SPACE_SM }) {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Column({ space: 2 }) {
          Text(this.tab.title)
            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
          Text(`${this.displayedEventCount} event(s) · ${this.tab.hostId}`)
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.arrow_clockwise'))
            .fontSize(15)
            .fontColor([this.store.palette.iconPrimary])
        }
        .width(32)
        .height(32)
        .padding(0)
        .backgroundColor(this.store.palette.subtleSurface)
        .accessibilityText('Refresh subagent timeline')
        .onClick(() => {
          this.manualRefresh();
        })
      }
      .width('100%')
      .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD, top: OrbitTokens.SPACE_SM })
      .alignItems(VerticalAlign.Center)

      if (this.displayedEventCount === 0) {
        Column({ space: OrbitTokens.SPACE_SM }) {
          Blank()
          SymbolGlyph($r('sys.symbol.person_2'))
            .fontSize(26)
            .fontColor([this.store.palette.fgMuted])
          Text('No subagent timeline events loaded yet.')
            .attributeModifier(orbitTextBody(this.store.palette.fgSecondary))
          Blank()
        }
        .width('100%')
        .layoutWeight(1)
        .alignItems(HorizontalAlign.Center)
      } else if (this.boundSession !== null) {
        OrbitScopedStageTurnList({
          session: this.boundSession,
          renderMarkdown: true
        })
      } else {
        List({ space: OrbitTokens.SPACE_SM }) {
          ForEach(
            this.providerItems,
            (item: TimelineItemDto) => {
              ListItem() {
                Column({ space: OrbitTokens.SPACE_XS }) {
                  Row({ space: OrbitTokens.SPACE_XS }) {
                    Text(item.item_type.replace(/_/g, ' '))
                      .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Medium))
                    Blank()
                    Text(`#${item.seq}`)
                      .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                  }
                  .width('100%')

                  if (item.item_type === 'assistant_message') {
                    RichMarkdownView({
                      blocks: item.blocks ?? [],
                      fallbackText: item.text ?? ''
                    })
                  } else {
                    Text(summarizeScopedItem(item))
                      .attributeModifier(
                        item.item_type === 'tool_call'
                          ? orbitTextMono(this.store.palette.fgPrimary)
                          : orbitTextBody(this.store.palette.fgPrimary)
                      )
                      .width('100%')
                      .copyOption(CopyOptions.LocalDevice)
                  }
                }
                .width('100%')
                .padding(OrbitTokens.SPACE_MD)
                .borderRadius(OrbitTokens.RADIUS_MD)
                .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
              }
            },
            (item: TimelineItemDto, idx: number): string =>
              `sub_ev_${this.lastTargetKey}_${item.item_type}_${item.seq}_${idx}_${orbitTextHashSig(summarizeScopedItem(item))}`
          )
        }
        .width('100%')
        .layoutWeight(1)
        .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
        .scrollBar(BarState.Auto)
        .edgeEffect(EdgeEffect.Spring)
      }

      if (this.effectiveChildAgentId !== null) {
        Row({ space: OrbitTokens.SPACE_XS }) {
          TextInput({ placeholder: 'Steer subagent…', text: this.steerText })
            .attributeModifier(
              orbitInputSubhead(
                this.store.palette.fgPrimary,
                this.store.palette.fgMuted,
                this.store.palette.recessed
              )
            )
            .height(38)
            .borderRadius(OrbitTokens.RADIUS_XL)
            .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
            .layoutWeight(1)
            .onFocus(() => {
              this.store.shellStore.setTextInputFocused(true);
            })
            .onBlur(() => {
              this.store.shellStore.setTextInputFocused(false);
            })
            .onChange((val: string) => {
              this.steerText = val;
            })
            .onSubmit(() => {
              this.submitSteer();
            })

          Button('Steer', { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(orbitButtonMeta(this.store.palette.onAccent, this.store.palette.accentPrimaryFill))
            .height(38)
            .padding({ left: 12, right: 12 })
            .enabled(this.steerText.trim().length > 0)
            .onClick(() => {
              this.submitSteer();
            })
        }
        .width('100%')
        .padding({
          left: OrbitTokens.SPACE_MD,
          right: OrbitTokens.SPACE_MD,
          bottom: Math.max(OrbitTokens.SPACE_MD, this.store.safeBottomVp)
        })
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.canvas)
  }
}

/**
 * Sibling Peer inspector tab (`kind === 'peer'`).
 * Strictly isolated by target identity (`scopeKey`, `tabId`, `hostId`, `agentId`):
 * - Clears draft & rebinds session cleanly on switch (P1-1).
 * - Subscribes once and observes `boundSession.commitRevision` (P1-4).
 */
@ComponentV2
export struct OrbitScopedPeerTabView {
  @Param @Require tab: OrbitSidePanelTab;
  @Param scopeKey: string = '';

  @Local private boundSession: OrbitStageSession | null = null;
  @Local private draftMsg: string = '';

  private loadSeq: number = 0;
  private lastTargetKey: string = '';
  @Local private store: OrbitHarmonyStore = orbitRuntime();

  aboutToAppear(): void {
    this.rebindPeer();
  }

  @Monitor('tab', 'scopeKey')
  onTargetChanged(_mon: IMonitor): void {
    const nextKey = this.computeTargetKey();
    if (nextKey !== this.lastTargetKey) {
      this.rebindPeer();
    }
  }

  private computeTargetKey(): string {
    return encodeSidePanelKeyTuple(['peer', this.scopeKey, this.tab.tabId, this.tab.hostId, this.tab.agentId ?? '']);
  }

  private rebindPeer(): void {
    // Increment generation FIRST and clear stale draft & session before early return (P1-1).
    this.loadSeq += 1;
    this.lastTargetKey = this.computeTargetKey();
    this.draftMsg = '';
    this.boundSession = null;

    const host = this.tab.hostId.trim();
    const agent = this.tab.agentId !== null ? this.tab.agentId.trim() : '';
    if (host.length === 0 || agent.length === 0) {
      return;
    }
    this.boundSession = this.store.bindScopedManagedSession(host, agent);
  }

  @Computed
  get displayedEventCount(): number {
    if (this.boundSession === null) {
      return 0;
    }
    const _commit = this.boundSession.commitRevision;
    if (_commit < 0) {
      return 0;
    }
    return countSessionRenderedItems(this.boundSession);
  }

  private sendToPeer(): void {
    const clean = this.draftMsg.trim();
    const targetHost = this.tab.hostId.trim();
    const targetAgent = this.tab.agentId !== null ? this.tab.agentId.trim() : '';
    if (clean.length === 0 || targetHost.length === 0 || targetAgent.length === 0) {
      return;
    }
    dismissHarmonyKeyboard(this.getUIContext());
    if (this.store.steerScopedAgent(targetHost, targetAgent, clean)) {
      this.draftMsg = '';
    }
  }

  build() {
    Column({ space: OrbitTokens.SPACE_SM }) {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Column({ space: 2 }) {
          Text(this.tab.title)
            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
          Text(`Peer · ${this.tab.hostId}`)
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
            .maxLines(1)
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        if (this.tab.agentId !== null && this.tab.agentId.length > 0) {
          Button('Focus', { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface))
            .height(28)
            .padding({ left: 10, right: 10 })
            .onClick(() => {
              if (this.tab.agentId !== null && this.tab.hostId.length > 0) {
                this.store.openSessionFromDeepLink(this.tab.hostId, this.tab.agentId, '');
              }
            })
        }
      }
      .width('100%')
      .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD, top: OrbitTokens.SPACE_SM })
      .alignItems(VerticalAlign.Center)

      if (this.displayedEventCount === 0 || this.boundSession === null) {
        Column({ space: OrbitTokens.SPACE_SM }) {
          Blank()
          Text('No peer messages loaded yet.')
            .attributeModifier(orbitTextBody(this.store.palette.fgSecondary))
          Blank()
        }
        .width('100%')
        .layoutWeight(1)
        .alignItems(HorizontalAlign.Center)
      } else {
        OrbitScopedStageTurnList({
          session: this.boundSession,
          renderMarkdown: false
        })
      }

      Row({ space: OrbitTokens.SPACE_XS }) {
        TextInput({ placeholder: 'Message peer…', text: this.draftMsg })
          .attributeModifier(
            orbitInputSubhead(
              this.store.palette.fgPrimary,
              this.store.palette.fgMuted,
              this.store.palette.recessed
            )
          )
          .height(38)
          .borderRadius(OrbitTokens.RADIUS_XL)
          .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
          .layoutWeight(1)
          .onFocus(() => {
            this.store.shellStore.setTextInputFocused(true);
          })
          .onBlur(() => {
            this.store.shellStore.setTextInputFocused(false);
          })
          .onChange((val: string) => {
            this.draftMsg = val;
          })
          .onSubmit(() => {
            this.sendToPeer();
          })

        Button('Send', { type: ButtonType.Capsule, stateEffect: true })
          .attributeModifier(orbitButtonMeta(this.store.palette.onAccent, this.store.palette.accentPrimaryFill))
          .height(38)
          .padding({ left: 12, right: 12 })
          .enabled(this.draftMsg.trim().length > 0)
          .onClick(() => {
            this.sendToPeer();
          })
      }
      .width('100%')
      .padding({
        left: OrbitTokens.SPACE_MD,
        right: OrbitTokens.SPACE_MD,
        bottom: Math.max(OrbitTokens.SPACE_MD, this.store.safeBottomVp)
      })
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.canvas)
  }
}

/**
 * Host-scoped Briefing tab (`kind === 'briefing'`).
 * Strictly consumes `tab.hostId` / `tab.agentId` via `getScopedBriefing` (P1-5),
 * separating panel tab semantics cleanly from the full-fleet `PersonalOverviewView`.
 */
@ComponentV2
export struct OrbitScopedBriefingTabView {
  @Param @Require tab: OrbitSidePanelTab;
  @Param scopeKey: string = '';

  @Local private scopedBriefing: PersonalAgentBriefingDto | null = null;
  @Local private boundHostId: string = '';
  @Local private boundAgentId: string = '';
  @Local private store: OrbitHarmonyStore = orbitRuntime();
  @Local private fleetStore: OrbitFleetStore = orbitRuntime().fleetStore;

  private loadSeq: number = 0;

  aboutToAppear(): void {
    this.refreshScopedBriefing();
  }

  @Monitor('tab', 'scopeKey', 'store.panelDataRevision', 'fleetStore.fleetRevision')
  onBriefingDependencyChanged(_mon: IMonitor): void {
    this.refreshScopedBriefing();
  }

  private refreshScopedBriefing(): void {
    this.loadSeq += 1;
    const host = this.tab.hostId.trim();
    const agent = this.tab.agentId !== null ? this.tab.agentId.trim() : '';
    this.boundHostId = host;
    this.boundAgentId = agent;
    if (host.length === 0) {
      this.scopedBriefing = null;
      return;
    }
    this.scopedBriefing = this.store.getScopedBriefing(host, agent.length > 0 ? agent : null);
  }

  @Computed
  get scopedDecisions(): BriefingDecisionDto[] {
    if (this.scopedBriefing === null || this.boundHostId.length === 0) {
      return [];
    }
    const out: BriefingDecisionDto[] = [];
    const list = this.scopedBriefing.needs_you;
    for (let i = 0; i < list.length; i++) {
      if (
        list[i].host_id === this.boundHostId &&
        (this.boundAgentId.length === 0 || list[i].agent_id === this.boundAgentId)
      ) {
        out.push(list[i]);
      }
    }
    return out;
  }

  @Computed
  get hostHeaderLabel(): string {
    if (this.scopedBriefing !== null && this.scopedBriefing.hosts.length > 0) {
      return this.scopedBriefing.hosts[0].name;
    }
    return this.boundHostId.length > 0 ? this.boundHostId : 'Host';
  }

  build() {
    Column({ space: OrbitTokens.SPACE_SM }) {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Column({ space: 2 }) {
          Text(`Briefing · ${this.hostHeaderLabel}`)
            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
          Text(
            `${this.scopedDecisions.length} waiting · ${this.scopedBriefing?.counts.running ?? 0} running · ${this.scopedBriefing?.counts.done_recent ?? 0} done`
          )
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
            .maxLines(1)
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.arrow_clockwise'))
            .fontSize(15)
            .fontColor([this.store.palette.iconPrimary])
        }
        .width(32)
        .height(32)
        .padding(0)
        .backgroundColor(this.store.palette.subtleSurface)
        .accessibilityText('Refresh host briefing')
        .onClick(() => {
          this.refreshScopedBriefing();
        })
      }
      .width('100%')
      .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD, top: OrbitTokens.SPACE_SM })
      .alignItems(VerticalAlign.Center)

      if (this.scopedDecisions.length === 0) {
        Column({ space: OrbitTokens.SPACE_SM }) {
          Blank()
          SymbolGlyph($r('sys.symbol.checkmark_circle'))
            .fontSize(26)
            .fontColor([this.store.palette.statusDirect])
          Text('All clear — no pending decisions on this host')
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
            this.scopedDecisions,
            (item: BriefingDecisionDto) => {
              ListItem() {
                Column({ space: OrbitTokens.SPACE_XS }) {
                  Text(item.agent_title.length > 0 ? item.agent_title : item.kind)
                    .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                    .maxLines(1)
                  Text(`${item.host_name} · ${item.summary}`)
                    .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                    .maxLines(2)
                    .textOverflow({ overflow: TextOverflow.Ellipsis })
                }
                .width('100%')
                .padding(OrbitTokens.SPACE_MD)
                .borderRadius(OrbitTokens.RADIUS_MD)
                .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
                .alignItems(HorizontalAlign.Start)
                .onClick(() => {
                  if (
                    item.host_id === this.boundHostId &&
                    (this.boundAgentId.length === 0 || item.agent_id === this.boundAgentId)
                  ) {
                    this.store.openSessionFromDeepLink(item.host_id, item.agent_id, 'permission_required');
                  }
                })
              }
            },
            (item: BriefingDecisionDto): string => `sb_dec_${this.boundHostId}_${item.key}`
          )
        }
        .width('100%')
        .layoutWeight(1)
        .padding({
          left: OrbitTokens.SPACE_MD,
          right: OrbitTokens.SPACE_MD,
          bottom: Math.max(OrbitTokens.SPACE_MD, this.store.safeBottomVp)
        })
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.canvas)
  }
}
