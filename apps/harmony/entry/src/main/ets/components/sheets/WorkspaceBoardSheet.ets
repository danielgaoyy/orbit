import {
  CapsuleSegmentButtonV2,
  LengthMetrics,
  SegmentButtonV2Items
} from '@kit.ArkUI';
import {
  AgentCardDto,
  WorkspaceEventRecordDto,
  formatAgentStatusLabel,
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
    this.store.triggerHaptic(6);
  }

  private saveBoardDraft(): void {
    dismissHarmonyKeyboard(this.getUIContext());
    this.store.saveWorkspaceBoard(this.boardDraft);
    this.isEditingBoard = false;
  }

  private submitSpawnPeer(): void {
    dismissHarmonyKeyboard(this.getUIContext());
    if (this.peerPromptDraft.trim().length === 0) {
      this.store.showToast('Enter instructions for the peer', true);
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
          placeholder: '## Goal\n\n## Contracts\n\n## Findings',
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
              Text('BOARD.md is empty. Tap Edit to record shared goals, API contracts, or findings for sibling agents.')
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
            Text('No sibling peers in this workspace')
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
            Text('Spawn a peer below to collaborate on the same Shared Blackboard.')
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
              Button({ type: ButtonType.Normal, stateEffect: true }) {
                Row({ space: OrbitTokens.SPACE_SM }) {
                  Row()
                    .width(8)
                    .height(8)
                    .borderRadius(4)
                    .backgroundColor(
                      peer.is_awaiting_permission
                        ? this.store.palette.statusWarning
                        : (peer.is_running ? this.store.palette.accentPrimary : this.store.palette.statusDirect)
                    )

                  Column({ space: 2 }) {
                    Row({ space: 6 }) {
                      if (peer.handle !== undefined && peer.handle !== null && peer.handle.length > 0) {
                        Text(`@${peer.handle}`)
                          .attributeModifier(orbitTextMono(this.store.palette.accentPrimary, FontWeight.Medium))
                      }
                      Text(peer.title.trim().length > 0 ? peer.title : peer.provider_label)
                        .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                        .maxLines(1)
                        .textOverflow({ overflow: TextOverflow.Ellipsis })
                        .layoutWeight(1)
                    }
                    .width('100%')
                    .alignItems(VerticalAlign.Center)

                    Text(`${formatProviderLabel(peer.provider)} · ${formatAgentStatusLabel(peer)}`)
                      .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                  }
                  .alignItems(HorizontalAlign.Start)
                  .layoutWeight(1)

                  if (peer.agent_id === this.store.selectedAgentId) {
                    Text('Current')
                      .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                      .padding({ left: 8, right: 8, top: 2, bottom: 2 })
                      .borderRadius(OrbitTokens.RADIUS_SM)
                      .backgroundColor(this.store.palette.subtleSurface)
                  } else {
                    SymbolGlyph($r('sys.symbol.chevron_right'))
                      .fontSize(13)
                      .fontColor([this.store.palette.fgMuted])
                  }
                }
                .width('100%')
                .alignItems(VerticalAlign.Center)
              }
              .width('100%')
              .padding(OrbitTokens.SPACE_LG)
              .backgroundColor(this.store.palette.listCard)
              .borderRadius(OrbitTokens.RADIUS_LG)
              .onClick(() => {
                this.store.selectAgent(peer.agent_id);
                this.store.showWorkspaceBoardSheet = false;
              })
            }
          },
          (peer: AgentCardDto): string => `peer_${peer.agent_id}_${peer.status.state}`
        )
      }

      ListItem() {
        OrbitSectionHeader({ title: 'Spawn Peer' })
      }

      ListItem() {
        Column({ space: OrbitTokens.SPACE_SM }) {
          TextInput({ placeholder: 'Optional @handle (e.g. reviewer)', text: this.peerHandleDraft })
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

          TextArea({ placeholder: 'Task brief for the new peer…', text: this.peerPromptDraft })
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
          Text('Shared Blackboard & Peers')
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
        .accessibilityText('Refresh board')
        .onClick(() => {
          this.store.refreshWorkspaceBoard();
          this.store.triggerHaptic(6);
        })
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)

      CapsuleSegmentButtonV2({
        items: this.tabItems,
        selectedIndex: this.activeTabIndex,
        onItemClicked: (idx: number) => {
          this.activeTabIndex = idx;
          this.store.triggerHaptic(6);
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
