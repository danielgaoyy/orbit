import {
  CapsuleSegmentButtonV2,
  LengthMetrics,
  SegmentButtonV2Items
} from '@kit.ArkUI';
import {
  AgentCardDto,
  WorkspaceEventRecordDto,
  formatCompactRelativeTime,
  formatProviderLabel
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitButtonSubhead,
  orbitInputSubhead,
  orbitSegmentSelectedBg,
  orbitSegmentSelectedFg,
  orbitTextBody,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead,
  orbitTextTitle
} from '../../theme/OrbitTokens';
import {
  OrbitSectionHeader,
  dismissHarmonyKeyboard
} from '../common/OrbitCommonControls';
import {
  OrbitAgentListItem,
  mapAgentCardToListStatus,
  resolveAgentEffortDisplay,
  resolveCompactAgentModelLabel
} from '../common/OrbitAgentListItem';

@ComponentV2
export struct WorkspaceBoardSheet {
  @Local activeTabIndex: number = 0;
  @Local isEditingBoard: boolean = false;
  @Local boardDraft: string = '';
  @Local peerHandleDraft: string = '';
  @Local peerPromptDraft: string = '';
  @Local tabItems: SegmentButtonV2Items = new SegmentButtonV2Items([
    { text: 'BOARD.md' },
    { text: 'Peers' }
  ]);

  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  aboutToAppear(): void {
    this.store.refreshWorkspaceBoard();
    this.boardDraft = this.store.workspaceBoardContent;
  }

  @Computed
  get workspaceTitle(): string {
    const info = this.store.activeWorkspaceInfo();
    if (info !== null && info.title.trim().length > 0) {
      return info.title.trim();
    }
    return 'Mission Workspace';
  }

  @Computed
  get peerList(): AgentCardDto[] {
    return this.store.activeWorkspacePeers();
  }

  private beginEditing(): void {
    this.boardDraft = this.store.workspaceBoardContent;
    this.isEditingBoard = true;
    this.store.triggerHapticPreset('selection');
  }

  private saveBoardDraft(): void {
    dismissHarmonyKeyboard(this.getUIContext());
    this.store.saveWorkspaceBoard(this.boardDraft);
    this.isEditingBoard = false;
  }

  private submitSpawnPeer(): void {
    dismissHarmonyKeyboard(this.getUIContext());
    if (this.peerPromptDraft.trim().length === 0) {
      this.store.showToastResource($r('app.string.toast_enter_peer_instructions'), true);
      return;
    }
    const ok = this.store.spawnWorkspacePeer(
      this.peerHandleDraft,
      this.peerPromptDraft,
      this.store.composerProvider
    );
    if (ok) {
      this.peerHandleDraft = '';
      this.peerPromptDraft = '';
    }
  }

  @Builder
  private renderBoardTab() {
    Column({ space: OrbitTokens.SPACE_SM }) {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Text(
          this.store.workspaceBoardRevision > 0
            ? `Rev ${this.store.workspaceBoardRevision}${this.store.workspaceBoardUpdatedBy.length > 0 ? ` · ${this.store.workspaceBoardUpdatedBy}` : ''}`
            : 'Shared Blackboard'
        )
          .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
          .layoutWeight(1)
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })

        if (!this.isEditingBoard) {
          if (this.store.workspaceBoardContent.length > 0) {
            Button('Copy', { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(
                orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface)
              )
              .height(32)
              .padding({ left: 10, right: 10 })
              .onClick(() => {
                this.store.copyToClipboard(this.store.workspaceBoardContent, 'Copied BOARD.md');
              })
          }

          Button('Edit', { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(
              orbitButtonMeta(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
            )
            .height(32)
            .padding({ left: 12, right: 12 })
            .onClick(() => {
              this.beginEditing();
            })
        } else {
          Button('Cancel', { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(
              orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface)
            )
            .height(32)
            .padding({ left: 10, right: 10 })
            .onClick(() => {
              dismissHarmonyKeyboard(this.getUIContext());
              this.isEditingBoard = false;
            })

          Button('Save', { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(
              orbitButtonMeta(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
            )
            .height(32)
            .padding({ left: 12, right: 12 })
            .onClick(() => {
              this.saveBoardDraft();
            })
        }
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)

      if (this.isEditingBoard) {
        TextArea({
          placeholder: $r('app.string.board_editor_placeholder'),
          text: this.boardDraft
        })
          .fontSize(OrbitTokens.FONT_META_RES)
          .fontFamily(OrbitTokens.FAMILY_MONO)
          .fontColor(this.store.palette.fgPrimary)
          .placeholderColor(this.store.palette.fgMuted)
          .backgroundColor(this.store.palette.listCard)
          .borderRadius(OrbitTokens.RADIUS_LG)
          .padding(OrbitTokens.SPACE_MD)
          .width('100%')
          .layoutWeight(1)
          .onChange((val: string) => {
            this.boardDraft = val;
          })
      } else {
        Scroll() {
          Column({ space: OrbitTokens.SPACE_SM }) {
            if (this.store.workspaceBoardContent.trim().length === 0) {
              Text($r('app.string.board_empty_hint'))
                .attributeModifier(orbitTextSubhead(this.store.palette.fgMuted))
                .width('100%')
            } else {
              Text(this.store.workspaceBoardContent)
                .attributeModifier(orbitTextBody(this.store.palette.fgPrimary))
                .copyOption(CopyOptions.LocalDevice)
                .width('100%')
            }
          }
          .width('100%')
          .padding(OrbitTokens.SPACE_LG)
          .alignItems(HorizontalAlign.Start)
        }
        .width('100%')
        .layoutWeight(1)
        .backgroundColor(this.store.palette.listCard)
        .borderRadius(OrbitTokens.RADIUS_LG)
        .scrollBar(BarState.Auto)
        .edgeEffect(EdgeEffect.Spring)
      }
    }
    .width('100%')
    .layoutWeight(1)
  }

  @Builder
  private renderPeersTab() {
    List({ space: OrbitTokens.SPACE_MD }) {
      ListItem() {
        OrbitSectionHeader({ title: `Workspace Peers (${this.peerList.length})`, isFirst: true })
      }

      if (this.peerList.length === 0) {
        ListItem() {
          Column({ space: OrbitTokens.SPACE_XS }) {
            Text($r('app.string.board_no_peers_title'))
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
            Text($r('app.string.board_no_peers_subtitle'))
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
          }
          .width('100%')
          .padding(OrbitTokens.SPACE_LG)
          .backgroundColor(this.store.palette.listCard)
          .borderRadius(OrbitTokens.RADIUS_LG)
          .alignItems(HorizontalAlign.Start)
        }
      } else {
        ForEach(
          this.peerList,
          (peer: AgentCardDto) => {
            ListItem() {
              OrbitAgentListItem({
                variant: 'card',
                title: (peer.handle !== undefined && peer.handle !== null && peer.handle.length > 0)
                  ? `@${peer.handle} · ${peer.title.trim().length > 0 ? peer.title : peer.provider_label}`
                  : (peer.title.trim().length > 0 ? peer.title : peer.provider_label),
                status: mapAgentCardToListStatus(peer),
                avatarSeedKey: peer.agent_id,
                provider: peer.provider,
                modelLabel: resolveCompactAgentModelLabel(
                  peer,
                  this.store.currentHostDetail?.provider_catalogs ?? []
                ),
                effortLabel: resolveAgentEffortDisplay(peer),
                agentLabel: formatProviderLabel(peer.provider),
                isSelected: peer.agent_id === this.store.selectedAgentId,
                titleTrailingText: peer.agent_id === this.store.selectedAgentId ? 'Current' : '',
                onItemClick: () => {
                  this.store.selectAgent(peer.agent_id);
                  this.store.showWorkspaceBoardSheet = false;
                }
              })
            }
          },
          (peer: AgentCardDto): string => `peer_${peer.agent_id}`
        )
      }

      ListItem() {
        OrbitSectionHeader({ title: 'Spawn Peer' })
      }

      ListItem() {
        Column({ space: OrbitTokens.SPACE_SM }) {
          TextInput({ placeholder: $r('app.string.board_peer_handle_placeholder'), text: this.peerHandleDraft })
            .attributeModifier(
              orbitInputSubhead(
                this.store.palette.fgPrimary,
                this.store.palette.fgMuted,
                this.store.palette.subtleSurface
              )
            )
            .height(40)
            .borderRadius(OrbitTokens.RADIUS_MD)
            .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
            .width('100%')
            .onChange((val: string) => {
              this.peerHandleDraft = val;
            })

          TextArea({ placeholder: $r('app.string.board_peer_brief_placeholder'), text: this.peerPromptDraft })
            .fontSize(OrbitTokens.FONT_SUBHEAD_RES)
            .fontColor(this.store.palette.fgPrimary)
            .placeholderColor(this.store.palette.fgMuted)
            .backgroundColor(this.store.palette.subtleSurface)
            .borderRadius(OrbitTokens.RADIUS_MD)
            .constraintSize({ minHeight: 68, maxHeight: 120 })
            .width('100%')
            .onChange((val: string) => {
              this.peerPromptDraft = val;
            })

          Button('Spawn Workspace Peer', { type: ButtonType.Normal, stateEffect: true })
            .attributeModifier(
              orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
            )
            .width('100%')
            .height(OrbitTokens.HIT_MIN)
            .borderRadius(OrbitTokens.RADIUS_MD)
            .onClick(() => {
              this.submitSpawnPeer();
            })
        }
        .width('100%')
        .padding(OrbitTokens.SPACE_LG)
        .backgroundColor(this.store.palette.listCard)
        .borderRadius(OrbitTokens.RADIUS_LG)
      }

      if (this.store.workspaceEvents.length > 0) {
        ListItem() {
          OrbitSectionHeader({ title: 'Recent Activity' })
        }

        ForEach(
          this.store.workspaceEvents,
          (ev: WorkspaceEventRecordDto) => {
            ListItem() {
              Row({ space: OrbitTokens.SPACE_SM }) {
                Column({ space: 2 }) {
                  Text(ev.summary)
                    .attributeModifier(orbitTextMeta(this.store.palette.fgPrimary))
                    .maxLines(2)
                    .textOverflow({ overflow: TextOverflow.Ellipsis })
                  Text(
                    `${ev.agent_handle !== undefined && ev.agent_handle !== null ? `@${ev.agent_handle} · ` : ''}${formatCompactRelativeTime(ev.created_at)}`
                  )
                    .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                }
                .alignItems(HorizontalAlign.Start)
                .layoutWeight(1)
              }
              .width('100%')
              .padding({
                left: OrbitTokens.SPACE_MD,
                right: OrbitTokens.SPACE_MD,
                top: OrbitTokens.SPACE_SM,
                bottom: OrbitTokens.SPACE_SM
              })
              .backgroundColor(this.store.palette.listCard)
              .borderRadius(OrbitTokens.RADIUS_MD)
            }
          },
          (ev: WorkspaceEventRecordDto): string => `ws_ev_${ev.seq}`
        )
      }
    }
    .width('100%')
    .layoutWeight(1)
    .scrollBar(BarState.Auto)
    .edgeEffect(EdgeEffect.Spring)
    .onScrollStart(() => {
      dismissHarmonyKeyboard(this.getUIContext());
    })
  }

  build() {
    Column({ space: OrbitTokens.SPACE_MD }) {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Column({ space: 2 }) {
          Text(this.workspaceTitle)
            .attributeModifier(orbitTextTitle(this.store.palette.fgPrimary, FontWeight.Bold))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
          Text($r('app.string.board_sheet_title'))
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.arrow_clockwise'))
            .fontSize(17)
            .fontColor([this.store.palette.iconPrimary])
        }
        .width(OrbitTokens.HIT_COMPACT)
        .height(OrbitTokens.HIT_COMPACT)
        .padding(0)
        .backgroundColor(this.store.palette.subtleSurface)
        .accessibilityText($r('app.string.board_refresh_a11y'))
        .onClick(() => {
          this.store.refreshWorkspaceBoard();
          this.store.triggerHapticPreset('selection');
        })
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)

      CapsuleSegmentButtonV2({
        items: this.tabItems,
        selectedIndex: this.activeTabIndex,
        onItemClicked: (idx: number) => {
          this.activeTabIndex = idx;
          this.store.triggerHapticPreset('selection');
        },
        itemFontSize: LengthMetrics.fp(OrbitTokens.FONT_META),
        itemSelectedFontSize: LengthMetrics.fp(OrbitTokens.FONT_META),
        itemSelectedBackgroundColor: orbitSegmentSelectedBg(this.store.palette),
        itemSelectedFontColor: orbitSegmentSelectedFg(this.store.palette),
        itemMaxFontScale: 1.3,
        itemMinHeight: LengthMetrics.vp(36)
      })
        .width('100%')

      if (this.activeTabIndex === 0) {
        this.renderBoardTab()
      } else {
        this.renderPeersTab()
      }
    }
    .width('100%')
    .height('100%')
    .padding({
      left: OrbitTokens.SPACE_LG,
      right: OrbitTokens.SPACE_LG,
      top: OrbitTokens.SPACE_LG,
      bottom: this.store.keyboardHeightVp > 0
        ? OrbitTokens.SPACE_MD
        : Math.max(OrbitTokens.SPACE_LG, this.store.safeBottomVp)
    })
    .backgroundColor(this.store.palette.drawer)
  }
}
