import { SymbolGlyphModifier } from '@kit.ArkUI';
import {
  HostCardDto,
  SidebarConversationDto,
  SidebarProjectGroupDto,
  basenameOfPath,
  cwdPathsEquivalent,
  formatCompactRelativeTime,
  formatCompactTopBarConnectionLabel
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import {
  OrbitTokens,
  orbitButtonSubhead,
  orbitTextMeta,
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import {
  dismissHarmonyKeyboard,
  resolveHostStatusColor,
  resolveSelectedHost
} from '../common/OrbitCommonControls';
import {
  MAX_VISIBLE_THREADS_PER_PROJECT,
  filterSidebarConversationsForHost,
  filterSidebarProjectsForHost
} from './FleetProjectGrouping';

@ComponentV2
export struct FleetSidebar {
  @Local expandedMoreProjectIds: string[] = [];

  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  @Computed
  get normalizedQuery(): string {
    return this.store.sidebarSearchQuery.trim().toLowerCase();
  }

  @Computed
  get selectedHostCard(): HostCardDto | null {
    return resolveSelectedHost(this.store);
  }

  @Computed
  get needsAttentionList(): SidebarConversationDto[] {
    if (this.normalizedQuery.length > 0) {
      return [];
    }
    return filterSidebarConversationsForHost(
      this.store.sidebarView?.needs_attention,
      this.store.selectedHostId
    );
  }

  @Computed
  get pinnedList(): SidebarConversationDto[] {
    if (this.normalizedQuery.length > 0) {
      return [];
    }
    return filterSidebarConversationsForHost(
      this.store.sidebarView?.pinned_conversations,
      this.store.selectedHostId
    );
  }

  @Computed
  get unifiedProjects(): SidebarProjectGroupDto[] {
    return filterSidebarProjectsForHost(
      this.store.sidebarView,
      this.store.selectedHostId
    );
  }

  private isProjectMoreExpanded(projectId: string): boolean {
    return this.expandedMoreProjectIds.indexOf(projectId) >= 0;
  }

  private toggleProjectMoreExpanded(projectId: string): void {
    if (this.isProjectMoreExpanded(projectId)) {
      this.expandedMoreProjectIds = this.expandedMoreProjectIds.filter((id: string) => id !== projectId);
    } else {
      this.expandedMoreProjectIds = this.expandedMoreProjectIds.concat([projectId]);
    }
    this.store.triggerHaptic(6);
  }

  private hasCustomSortOrFilter(): boolean {
    return (
      this.store.sidebarProjectSort !== 'recent' ||
      this.store.sidebarThreadFilter !== 'all'
    );
  }

  private hostConnectionSummary(host: HostCardDto): string {
    const conn = formatCompactTopBarConnectionLabel(host.connection_state, host.via_ssh_tunnel);
    return host.last_ping_ms !== null ? `${conn} · ${host.last_ping_ms}ms` : conn;
  }

  private visibleThreadsForGroup(group: SidebarProjectGroupDto): SidebarConversationDto[] {
    if (this.normalizedQuery.length > 0 || this.isProjectMoreExpanded(group.id)) {
      return group.conversations;
    }
    return group.conversations.slice(0, MAX_VISIBLE_THREADS_PER_PROJECT);
  }

  @Builder
  private renderFilterSortMenu() {
    Menu() {
      MenuItemGroup({ header: 'Sort by' }) {
        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.clock'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Recent',
          symbolEndIcon: this.store.sidebarProjectSort === 'recent'
            ? new SymbolGlyphModifier($r('sys.symbol.checkmark')).fontColor([this.store.palette.accentPrimary])
            : undefined
        })
          .onClick(() => {
            this.store.setSidebarProjectSort('recent');
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.textformat'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Name',
          symbolEndIcon: this.store.sidebarProjectSort === 'name'
            ? new SymbolGlyphModifier($r('sys.symbol.checkmark')).fontColor([this.store.palette.accentPrimary])
            : undefined
        })
          .onClick(() => {
            this.store.setSidebarProjectSort('name');
          })
      }

      MenuItemGroup({ header: 'Show' }) {
        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.message'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'All Conversations',
          symbolEndIcon: this.store.sidebarThreadFilter === 'all'
            ? new SymbolGlyphModifier($r('sys.symbol.checkmark')).fontColor([this.store.palette.accentPrimary])
            : undefined
        })
          .onClick(() => {
            this.store.setSidebarThreadFilter('all');
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.dot_circle'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Active & Unread',
          symbolEndIcon: this.store.sidebarThreadFilter === 'active_unread'
            ? new SymbolGlyphModifier($r('sys.symbol.checkmark')).fontColor([this.store.palette.accentPrimary])
            : undefined
        })
          .onClick(() => {
            this.store.setSidebarThreadFilter('active_unread');
          })
      }
    }
  }

  @Builder
  private renderHostSwitcherMenu() {
    Menu() {
      MenuItemGroup({ header: 'Hosts' }) {
        ForEach(
          this.store.hosts,
          (host: HostCardDto) => {
            MenuItem({
              symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.dot_circle'))
                .fontColor([resolveHostStatusColor(host, this.store)]),
              content: host.display_name,
              labelInfo: this.hostConnectionSummary(host),
              symbolEndIcon: host.host_id === this.store.selectedHostId
                ? new SymbolGlyphModifier($r('sys.symbol.checkmark')).fontColor([this.store.palette.accentPrimary])
                : undefined
            })
              .labelFontColor(this.store.palette.fgMuted)
              .onClick(() => {
                this.store.selectHost(host.host_id);
                this.store.triggerHaptic(8);
              })
          },
          (host: HostCardDto) => `host_menu_${host.host_id}`
        )
      }

      MenuItemGroup() {
        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.qrcode'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Pair New Host'
        })
          .onClick(() => {
            this.store.openHostSettings();
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.bolt'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Connect Local Host'
        })
          .onClick(() => {
            this.store.discoverAndPairLocalDaemon();
          })
      }
    }
  }

  @Builder
  private renderProjectContextMenu(group: SidebarProjectGroupDto) {
    Menu() {
      MenuItemGroup() {
        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.plus'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'New Session Here'
        })
          .onClick(() => {
            this.store.startNewSessionDraft(group.primary_cwd);
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.folder'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Select Workspace'
        })
          .onClick(() => {
            this.store.startNewSessionDraft(group.primary_cwd);
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.arrow_left_arrow_right'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Diff Review'
        })
          .onClick(() => {
            this.store.inspectGitDiffForCwd(group.primary_cwd);
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.code_square'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Terminal'
        })
          .onClick(() => {
            this.store.openPtyTerminalInCwd(group.primary_cwd);
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier(
            (group.pinned_project_id ?? '').length > 0 ? $r('sys.symbol.pin_slash') : $r('sys.symbol.pin')
          ).fontColor([this.store.palette.iconPrimary]),
          content: (group.pinned_project_id ?? '').length > 0 ? 'Unpin Project' : 'Pin Project'
        })
          .onClick(() => {
            const pinnedProjId = group.pinned_project_id ?? '';
            if (pinnedProjId.length > 0) {
              this.store.removePinnedProject(pinnedProjId);
            } else {
              this.store.pinDirectoryAsProject(group.primary_cwd);
            }
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.square_and_square'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Copy Path'
        })
          .onClick(() => {
            this.store.copyToClipboard(group.primary_cwd, 'Copied path');
          })
      }
    }
  }

  @Builder
  private renderSessionContextMenu(conv: SidebarConversationDto) {
    Menu() {
      MenuItemGroup() {
        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier(
            conv.is_pinned ? $r('sys.symbol.pin_slash') : $r('sys.symbol.pin')
          ).fontColor([this.store.palette.iconPrimary]),
          content: conv.is_pinned ? 'Unpin Conversation' : 'Pin Conversation'
        })
          .onClick(() => {
            this.store.togglePinThread(conv.host_id, conv.agent.agent_id);
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.dot_circle'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Mark Unread'
        })
          .onClick(() => {
            this.store.markAgentUnread(conv.host_id, conv.agent.agent_id);
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.plus'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'New Session in Workspace'
        })
          .onClick(() => {
            this.store.startNewSessionDraft(conv.agent.cwd);
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.arrow_left_arrow_right'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Diff Review'
        })
          .onClick(() => {
            this.store.inspectGitDiffForCwd(conv.agent.cwd);
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.code_square'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Terminal'
        })
          .onClick(() => {
            this.store.openPtyTerminalInCwd(conv.agent.cwd);
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.square_and_square'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Copy Workspace Path'
        })
          .onClick(() => {
            this.store.copyToClipboard(conv.agent.cwd, 'Copied path');
          })
      }

      MenuItemGroup() {
        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.archivebox'))
            .fontColor([this.store.palette.statusDanger]),
          content: 'Archive'
        })
          .contentFontColor(this.store.palette.statusDanger)
          .onClick(() => {
            this.store.archiveSession(conv.agent.agent_id);
          })
      }
    }
  }

  @Builder
  private renderSwipeArchiveButton(agentId: string) {
    Row() {
      Button({ type: ButtonType.Normal, stateEffect: true }) {
        SymbolGlyph($r('sys.symbol.archivebox'))
          .fontSize(16)
          .renderingStrategy(SymbolRenderingStrategy.SINGLE)
          .fontColor([$r('sys.color.font_on_primary')])
      }
      .height('100%')
      .constraintSize({ minWidth: OrbitTokens.HIT_MIN })
      .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
      .backgroundColor(this.store.palette.statusDanger)
      .borderRadius(OrbitTokens.RADIUS_MD)
      .accessibilityText('Archive session')
      .onClick(() => {
        this.store.archiveSession(agentId);
      })
    }
    .height('100%')
    .padding({ left: OrbitTokens.SPACE_XS })
  }

  @Builder
  private renderConversationRow(
    conv: SidebarConversationDto,
    showWorkspaceHint: boolean,
    indentLeft: number
  ) {
    ListItem() {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Row() {
          if (conv.agent.is_awaiting_permission) {
            SymbolGlyph($r('sys.symbol.exclamationmark_triangle_fill'))
              .fontSize(14)
              .fontColor([this.store.palette.statusWarning])
          } else if (conv.agent.is_running) {
            LoadingProgress()
              .width(16)
              .height(16)
              .color(this.store.palette.iconSecondary)
          } else if (conv.is_unread) {
            Row()
              .width(7)
              .height(7)
              .borderRadius(3.5)
              .backgroundColor(this.store.palette.unreadBadge)
          }
        }
        .width(20)
        .justifyContent(FlexAlign.Center)
        .alignItems(VerticalAlign.Center)

        Column({ space: 2 }) {
          Text(conv.agent.title.trim().length > 0 ? conv.agent.title : conv.agent.provider_label)
            .attributeModifier(
              orbitTextSubhead(
                this.store.palette.fgPrimary,
                !this.store.isNewSessionDraft && this.store.selectedAgentId === conv.agent.agent_id
                  ? FontWeight.Medium
                  : FontWeight.Normal
              )
            )
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })

          if (showWorkspaceHint) {
            Text(basenameOfPath(conv.agent.cwd))
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
          }
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        if (formatCompactRelativeTime(conv.agent.updated_at_rfc3339).length > 0) {
          Text(formatCompactRelativeTime(conv.agent.updated_at_rfc3339))
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
            .maxLines(1)
        }
      }
      .width('100%')
      .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
      .padding({
        left: indentLeft,
        right: OrbitTokens.SPACE_MD,
        top: 6,
        bottom: 6
      })
      .backgroundColor(
        !this.store.isNewSessionDraft && this.store.selectedAgentId === conv.agent.agent_id
          ? this.store.palette.subtleSurface
          : Color.Transparent
      )
      .borderRadius(OrbitTokens.RADIUS_MD)
      .alignItems(VerticalAlign.Center)
      .onClick(() => {
        dismissHarmonyKeyboard(this.getUIContext());
        this.store.selectAgent(conv.agent.agent_id);
      })
      .bindContextMenu(
        () => {
          this.renderSessionContextMenu(conv);
        },
        ResponseType.LongPress
      )
    }
    .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM, top: 1, bottom: 1 })
    .swipeAction({
      end: {
        builder: () => {
          this.renderSwipeArchiveButton(conv.agent.agent_id);
        }
      }
    })
  }

  @Builder
  private renderProjectHeaderRow(group: SidebarProjectGroupDto) {
    ListItem() {
      Row({ space: 6 }) {
        SymbolGlyph(
          this.normalizedQuery.length === 0 && this.store.isProjectCollapsed(group.id)
            ? $r('sys.symbol.chevron_right')
            : $r('sys.symbol.chevron_down')
        )
          .fontSize(12)
          .fontColor([this.store.palette.fgMuted])

        SymbolGlyph($r('sys.symbol.folder'))
          .fontSize(16)
          .fontColor([
            this.store.isNewSessionDraft && cwdPathsEquivalent(group.primary_cwd, this.store.activeCwd)
              ? this.store.palette.accentPrimary
              : this.store.palette.iconSecondary
          ])

        Text(group.name)
          .attributeModifier(
            orbitTextSubhead(
              this.store.palette.fgPrimary,
              this.store.isNewSessionDraft && cwdPathsEquivalent(group.primary_cwd, this.store.activeCwd)
                ? FontWeight.Bold
                : FontWeight.Medium
            )
          )
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .layoutWeight(1)

        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.plus'))
            .fontSize(16)
            .renderingStrategy(SymbolRenderingStrategy.SINGLE)
            .fontColor([this.store.palette.iconSecondary])
        }
        .width(OrbitTokens.HIT_COMPACT)
        .height(OrbitTokens.HIT_COMPACT)
        .padding(0)
        .backgroundColor(Color.Transparent)
        .accessibilityText(`New session in ${group.name}`)
        .onClick(() => {
          dismissHarmonyKeyboard(this.getUIContext());
          this.store.startNewSessionDraft(group.primary_cwd);
        })
      }
      .width('100%')
      .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
      .padding({ left: 8, right: 4 })
      .backgroundColor(
        this.store.isNewSessionDraft && cwdPathsEquivalent(group.primary_cwd, this.store.activeCwd)
          ? this.store.palette.subtleSurface
          : Color.Transparent
      )
      .borderRadius(OrbitTokens.RADIUS_MD)
      .alignItems(VerticalAlign.Center)
      .onClick(() => {
        dismissHarmonyKeyboard(this.getUIContext());
        this.store.toggleProjectCollapsed(group.id);
      })
      .bindContextMenu(
        () => {
          this.renderProjectContextMenu(group);
        },
        ResponseType.LongPress
      )
    }
    .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM, top: 1, bottom: 1 })
  }

  @Builder
  private renderHostSwitcherPillBody() {
    Row()
      .width(8)
      .height(8)
      .borderRadius(4)
      .backgroundColor(resolveHostStatusColor(this.selectedHostCard, this.store))

    Text(this.selectedHostCard?.display_name ?? 'No Host')
      .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold))
      .maxLines(1)
      .textOverflow({ overflow: TextOverflow.Ellipsis })

    if (this.selectedHostCard !== null) {
      Text(this.hostConnectionSummary(this.selectedHostCard))
        .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
        .layoutWeight(1)
    } else {
      Text('Tap to pair host')
        .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
        .maxLines(1)
        .layoutWeight(1)
    }

    if (this.store.otherHostsPendingApprovals() > 0) {
      Text(`${this.store.otherHostsPendingApprovals()}`)
        .attributeModifier(orbitTextMeta(this.store.palette.statusWarning, FontWeight.Bold))
        .padding({ left: 6, right: 6, top: 2, bottom: 2 })
        .borderRadius(OrbitTokens.RADIUS_MD)
        .backgroundColor(this.store.palette.statusWarningSoft)
    }

    SymbolGlyph($r('sys.symbol.chevron_down'))
      .fontSize(12)
      .fontColor([this.store.palette.fgMuted])
  }

  build() {
    Column() {
      // 1. Native HarmonyOS 7 Search + Compose Action Row
      Row({ space: OrbitTokens.SPACE_SM }) {
        Search({ value: this.store.sidebarSearchQuery, placeholder: 'Search' })
          .layoutWeight(1)
          .height(OrbitTokens.HIT_COMPACT)
          .backgroundColor(this.store.palette.subtleSurface)
          .placeholderColor(this.store.palette.fgMuted)
          .fontColor(this.store.palette.fgPrimary)
          .textFont({ size: OrbitTokens.FONT_SUBHEAD_RES })
          .placeholderFont({ size: OrbitTokens.FONT_SUBHEAD_RES })
          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
          .onChange((val: string) => {
            this.store.setSidebarSearchQuery(val);
          })

        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.square_and_pencil'))
            .fontSize(19)
            .renderingStrategy(SymbolRenderingStrategy.SINGLE)
            .fontColor([
              this.store.selectedHostId.length > 0
                ? this.store.palette.iconPrimary
                : this.store.palette.fgMuted
            ])
        }
        .width(OrbitTokens.HIT_COMPACT)
        .height(OrbitTokens.HIT_COMPACT)
        .padding(0)
        .backgroundColor(this.store.palette.subtleSurface)
        .enabled(this.store.selectedHostId.length > 0)
        .accessibilityText('New session')
        .onClick(() => {
          dismissHarmonyKeyboard(this.getUIContext());
          if (this.store.selectedHostId.length > 0) {
            this.store.startNewSessionDraft();
          }
        })
      }
      .width('100%')
      .padding({
        left: OrbitTokens.SPACE_MD,
        right: OrbitTokens.SPACE_MD,
        top: OrbitTokens.SPACE_SM,
        bottom: OrbitTokens.SPACE_SM
      })
      .alignItems(VerticalAlign.Center)

      // 2. Unified Projects -> Conversations Scroll Area
      if (this.store.hosts.length === 0) {
        Column({ space: OrbitTokens.SPACE_MD }) {
          Column({ space: OrbitTokens.SPACE_SM }) {
            Text('No Hosts Paired')
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold))
            Text('Pair a remote Orbit daemon or connect a local daemon to browse workspaces and sessions.')
              .attributeModifier(orbitTextSubhead(this.store.palette.fgSecondary, FontWeight.Normal))

            Button('Connect Local Daemon')
              .attributeModifier(
                orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
              )
              .width('100%')
              .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
              .borderRadius(OrbitTokens.RADIUS_MD)
              .onClick(() => {
                this.store.discoverAndPairLocalDaemon();
              })

            Button('Pair Remote Host (QR / URI)')
              .attributeModifier(
                orbitButtonSubhead(this.store.palette.fgPrimary, this.store.palette.subtleSurface)
              )
              .width('100%')
              .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
              .borderRadius(OrbitTokens.RADIUS_MD)
              .onClick(() => {
                this.store.scanQrAndPair(this.getUIContext().getHostContext());
              })
          }
          .width('100%')
          .padding(OrbitTokens.SPACE_LG)
          .backgroundColor(this.store.palette.listCard)
          .borderRadius(OrbitTokens.RADIUS_LG)
          .alignItems(HorizontalAlign.Start)
        }
        .width('100%')
        .layoutWeight(1)
        .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD, top: OrbitTokens.SPACE_LG })
      } else {
        List() {
          if (this.needsAttentionList.length > 0) {
            ListItem() {
              Text('Needs Attention')
                .attributeModifier(orbitTextMeta(this.store.palette.statusWarning, FontWeight.Medium))
                .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG, top: 6, bottom: 4 })
            }

            ForEach(
              this.needsAttentionList,
              (conv: SidebarConversationDto) => {
                this.renderConversationRow(conv, true, 12)
              },
              (conv: SidebarConversationDto) => `attn_${conv.host_id}_${conv.agent.agent_id}`
            )
          }

          if (this.pinnedList.length > 0) {
            ListItem() {
              Text('Pinned')
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Medium))
                .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG, top: 10, bottom: 4 })
            }

            ForEach(
              this.pinnedList,
              (conv: SidebarConversationDto) => {
                this.renderConversationRow(conv, true, 12)
              },
              (conv: SidebarConversationDto) => `pin_${conv.host_id}_${conv.agent.agent_id}`
            )
          }

          ListItem() {
            Row({ space: 4 }) {
              Text('Projects')
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Medium))
                .layoutWeight(1)

              Button({ type: ButtonType.Circle, stateEffect: true }) {
                SymbolGlyph($r('sys.symbol.sort'))
                  .fontSize(17)
                  .renderingStrategy(SymbolRenderingStrategy.SINGLE)
                  .fontColor([
                    this.hasCustomSortOrFilter()
                      ? this.store.palette.accentPrimary
                      : this.store.palette.iconSecondary
                  ])
              }
              .width(OrbitTokens.HIT_COMPACT)
              .height(OrbitTokens.HIT_COMPACT)
              .padding(0)
              .backgroundColor(Color.Transparent)
              .accessibilityText('Sort and filter conversations')
              .bindMenu(this.renderFilterSortMenu())

              Button({ type: ButtonType.Circle, stateEffect: true }) {
                SymbolGlyph($r('sys.symbol.folder_badge_plus'))
                  .fontSize(17)
                  .renderingStrategy(SymbolRenderingStrategy.SINGLE)
                  .fontColor([
                    this.store.showDirBrowser
                      ? this.store.palette.accentPrimary
                      : this.store.palette.iconSecondary
                  ])
              }
              .width(OrbitTokens.HIT_COMPACT)
              .height(OrbitTokens.HIT_COMPACT)
              .padding(0)
              .backgroundColor(Color.Transparent)
              .accessibilityText('Browse remote directory')
              .onClick(() => {
                dismissHarmonyKeyboard(this.getUIContext());
                if (this.store.windowWidthVp < OrbitTokens.BP_FOLDABLE_MIN) {
                  this.store.sidebarOpen = false;
                }
                this.store.browseDirectory(
                  this.store.activeCwd === '~' ? null : this.store.activeCwd
                );
              })
            }
            .width('100%')
            .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_SM, top: 8, bottom: 2 })
            .alignItems(VerticalAlign.Center)
          }

          if (this.unifiedProjects.length === 0) {
            ListItem() {
              Column({ space: OrbitTokens.SPACE_SM }) {
                Text(
                  this.normalizedQuery.length > 0
                    ? 'No matching projects or conversations.'
                    : this.store.sidebarThreadFilter === 'active_unread'
                      ? 'No active or unread conversations.'
                      : 'No repositories discovered yet.'
                )
                  .attributeModifier(orbitTextSubhead(this.store.palette.fgMuted, FontWeight.Normal))

                if (this.normalizedQuery.length > 0) {
                  Button('Clear search', { type: ButtonType.Normal, stateEffect: true })
                    .attributeModifier(orbitButtonSubhead(this.store.palette.accentPrimary, Color.Transparent))
                    .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                    .padding({ left: 0, right: OrbitTokens.SPACE_SM })
                    .onClick(() => {
                      this.store.setSidebarSearchQuery('');
                    })
                } else if (this.store.sidebarThreadFilter !== 'all') {
                  Button('Show all conversations', { type: ButtonType.Normal, stateEffect: true })
                    .attributeModifier(orbitButtonSubhead(this.store.palette.accentPrimary, Color.Transparent))
                    .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                    .padding({ left: 0, right: OrbitTokens.SPACE_SM })
                    .onClick(() => {
                      this.store.setSidebarThreadFilter('all');
                    })
                }
              }
              .width('100%')
              .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG, top: OrbitTokens.SPACE_MD })
              .alignItems(HorizontalAlign.Start)
            }
          } else {
            ForEach(
              this.unifiedProjects,
              (group: SidebarProjectGroupDto) => {
                ListItemGroup() {
                  this.renderProjectHeaderRow(group)

                  if (
                    this.normalizedQuery.length > 0 ||
                    !this.store.isProjectCollapsed(group.id)
                  ) {
                    ForEach(
                      this.visibleThreadsForGroup(group),
                      (conv: SidebarConversationDto) => {
                        this.renderConversationRow(conv, false, 20)
                      },
                      (conv: SidebarConversationDto) => `t_${group.id}_${conv.host_id}_${conv.agent.agent_id}`
                    )

                    if (
                      this.normalizedQuery.length === 0 &&
                      group.conversations.length > MAX_VISIBLE_THREADS_PER_PROJECT
                    ) {
                      ListItem() {
                        Button(
                          this.isProjectMoreExpanded(group.id)
                            ? 'Show less'
                            : `Show ${group.conversations.length - MAX_VISIBLE_THREADS_PER_PROJECT} more…`,
                          { type: ButtonType.Normal, stateEffect: true }
                        )
                          .attributeModifier(orbitButtonSubhead(this.store.palette.fgMuted, Color.Transparent))
                          .width('100%')
                          .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                          .padding({ left: 46, right: OrbitTokens.SPACE_MD })
                          .onClick(() => {
                            this.toggleProjectMoreExpanded(group.id);
                          })
                      }
                      .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
                    }
                  }
                }
              },
              (group: SidebarProjectGroupDto) => `proj_${group.id}`
            )
          }
        }
        .width('100%')
        .layoutWeight(1)
        .scrollBar(BarState.Auto)
        .edgeEffect(EdgeEffect.Spring)
        .padding({ bottom: OrbitTokens.SPACE_SM })
        .onScrollStart(() => {
          dismissHarmonyKeyboard(this.getUIContext());
        })
      }

      // 3. Thumb-Zone Bottom Bar: [ Host Switcher Capsule ] [ Settings Circle ]
      Row({ space: OrbitTokens.SPACE_SM }) {
        if (this.store.hosts.length > 0) {
          Button({ type: ButtonType.Capsule, stateEffect: true }) {
            Row({ space: OrbitTokens.SPACE_SM }) {
              this.renderHostSwitcherPillBody()
            }
            .width('100%')
            .alignItems(VerticalAlign.Center)
          }
          .layoutWeight(1)
          .height(OrbitTokens.HIT_COMPACT)
          .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
          .backgroundColor(this.store.palette.subtleSurface)
          .accessibilityText('Switch active host')
          .bindMenu(this.renderHostSwitcherMenu())
        } else {
          Button({ type: ButtonType.Capsule, stateEffect: true }) {
            Row({ space: OrbitTokens.SPACE_SM }) {
              this.renderHostSwitcherPillBody()
            }
            .width('100%')
            .alignItems(VerticalAlign.Center)
          }
          .layoutWeight(1)
          .height(OrbitTokens.HIT_COMPACT)
          .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
          .backgroundColor(this.store.palette.subtleSurface)
          .accessibilityText('Pair host')
          .onClick(() => {
            this.store.openHostSettings();
          })
        }

        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.gearshape'))
            .fontSize(19)
            .renderingStrategy(SymbolRenderingStrategy.SINGLE)
            .fontColor([this.store.palette.iconPrimary])
        }
        .width(OrbitTokens.HIT_COMPACT)
        .height(OrbitTokens.HIT_COMPACT)
        .padding(0)
        .backgroundColor(this.store.palette.subtleSurface)
        .accessibilityText('Settings and hosts')
        .onClick(() => {
          dismissHarmonyKeyboard(this.getUIContext());
          this.store.openHostSettings();
        })
      }
      .width('100%')
      .padding({
        left: OrbitTokens.SPACE_MD,
        right: OrbitTokens.SPACE_MD,
        top: OrbitTokens.SPACE_SM,
        bottom: 4
      })
      .alignItems(VerticalAlign.Center)
    }
    .width('100%')
    .height('100%')
    .padding({
      top: this.store.safeTopVp,
      bottom: Math.max(this.store.safeBottomVp, 8),
      left: this.store.safeLeftVp
    })
    .backgroundColor(this.store.palette.drawer)
  }
}
