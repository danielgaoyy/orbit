import { SymbolGlyphModifier } from '@kit.ArkUI';
import {
  AgentCardDto,
  HostCardDto,
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
  SidebarProjectGroup,
  buildSidebarNeedsAttention,
  buildSidebarPinnedAgents,
  buildSidebarUnifiedProjects
} from './FleetProjectGrouping';

@ComponentV2
export struct FleetSidebar {
  @Local searchQuery: string = '';
  @Local expandedMoreProjectIds: string[] = [];

  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  @Computed
  get normalizedQuery(): string {
    return this.searchQuery.trim().toLowerCase();
  }

  @Computed
  get selectedHostCard(): HostCardDto | null {
    return resolveSelectedHost(this.store);
  }

  @Computed
  get needsAttentionList(): AgentCardDto[] {
    return buildSidebarNeedsAttention(this.store.currentHostDetail, this.normalizedQuery);
  }

  @Computed
  get pinnedList(): AgentCardDto[] {
    const _seenRev = this.store.seenSeqRevision;
    if (_seenRev < 0) {
      return [];
    }
    return buildSidebarPinnedAgents(
      this.store,
      this.store.currentHostDetail,
      this.store.selectedHostId,
      this.normalizedQuery
    );
  }

  @Computed
  get unifiedProjects(): SidebarProjectGroup[] {
    const _seenRev = this.store.seenSeqRevision;
    if (_seenRev < 0) {
      return [];
    }
    return buildSidebarUnifiedProjects(
      this.store,
      this.store.currentHostDetail,
      this.store.selectedHostId,
      this.normalizedQuery
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

  private isAgentUnreadReactive(hostId: string, agent: AgentCardDto): boolean {
    const rev = this.store.seenSeqRevision;
    if (rev < 0 || hostId.length === 0) {
      return false;
    }
    return this.store.isThreadUnread(hostId, agent);
  }

  private visibleThreadsForGroup(group: SidebarProjectGroup): AgentCardDto[] {
    if (this.normalizedQuery.length > 0 || this.isProjectMoreExpanded(group.projectId)) {
      return group.threads;
    }
    return group.threads.slice(0, MAX_VISIBLE_THREADS_PER_PROJECT);
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
  private renderProjectContextMenu(group: SidebarProjectGroup) {
    Menu() {
      MenuItemGroup() {
        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.plus'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'New Session Here'
        })
          .onClick(() => {
            this.store.startNewSessionDraft(group.primaryDir);
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.folder'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Select Workspace'
        })
          .onClick(() => {
            this.store.startNewSessionDraft(group.primaryDir);
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.arrow_left_arrow_right'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Diff Review'
        })
          .onClick(() => {
            this.store.inspectGitDiffForCwd(group.primaryDir);
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.code_square'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Terminal'
        })
          .onClick(() => {
            this.store.openPtyTerminalInCwd(group.primaryDir);
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier(
            group.pinnedProjectId.length > 0 ? $r('sys.symbol.pin_slash') : $r('sys.symbol.pin')
          ).fontColor([this.store.palette.iconPrimary]),
          content: group.pinnedProjectId.length > 0 ? 'Unpin Project' : 'Pin Project'
        })
          .onClick(() => {
            if (group.pinnedProjectId.length > 0) {
              this.store.removePinnedProject(group.pinnedProjectId);
            } else {
              this.store.pinDirectoryAsProject(group.primaryDir);
            }
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.square_and_square'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Copy Path'
        })
          .onClick(() => {
            this.store.copyToClipboard(group.primaryDir, 'Copied path');
          })
      }
    }
  }

  @Builder
  private renderSessionContextMenu(agent: AgentCardDto) {
    Menu() {
      MenuItemGroup() {
        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier(
            this.store.isThreadPinned(this.store.selectedHostId, agent.agent_id)
              ? $r('sys.symbol.pin_slash')
              : $r('sys.symbol.pin')
          ).fontColor([this.store.palette.iconPrimary]),
          content: this.store.isThreadPinned(this.store.selectedHostId, agent.agent_id)
            ? 'Unpin Conversation'
            : 'Pin Conversation'
        })
          .onClick(() => {
            this.store.togglePinThread(this.store.selectedHostId, agent.agent_id);
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.plus'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'New Session in Workspace'
        })
          .onClick(() => {
            this.store.startNewSessionDraft(agent.cwd);
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.arrow_left_arrow_right'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Diff Review'
        })
          .onClick(() => {
            this.store.inspectGitDiffForCwd(agent.cwd);
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.code_square'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Terminal'
        })
          .onClick(() => {
            this.store.openPtyTerminalInCwd(agent.cwd);
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.square_and_square'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Copy Workspace Path'
        })
          .onClick(() => {
            this.store.copyToClipboard(agent.cwd, 'Copied path');
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
            this.store.archiveSession(agent.agent_id);
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
    agent: AgentCardDto,
    showWorkspaceHint: boolean,
    indentLeft: number
  ) {
    ListItem() {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Row() {
          if (agent.is_awaiting_permission) {
            SymbolGlyph($r('sys.symbol.exclamationmark_triangle_fill'))
              .fontSize(14)
              .fontColor([this.store.palette.statusWarning])
          } else if (agent.is_running) {
            LoadingProgress()
              .width(16)
              .height(16)
              .color(this.store.palette.iconSecondary)
          } else if (this.isAgentUnreadReactive(this.store.selectedHostId, agent)) {
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
          Text(agent.title.trim().length > 0 ? agent.title : agent.provider_label)
            .attributeModifier(
              orbitTextSubhead(
                this.store.palette.fgPrimary,
                !this.store.isNewSessionDraft && this.store.selectedAgentId === agent.agent_id
                  ? FontWeight.Medium
                  : FontWeight.Normal
              )
            )
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })

          if (showWorkspaceHint) {
            Text(basenameOfPath(agent.cwd))
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
          }
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        if (formatCompactRelativeTime(agent.updated_at_rfc3339).length > 0) {
          Text(formatCompactRelativeTime(agent.updated_at_rfc3339))
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
        !this.store.isNewSessionDraft && this.store.selectedAgentId === agent.agent_id
          ? this.store.palette.subtleSurface
          : Color.Transparent
      )
      .borderRadius(OrbitTokens.RADIUS_MD)
      .alignItems(VerticalAlign.Center)
      .onClick(() => {
        dismissHarmonyKeyboard(this.getUIContext());
        this.store.selectAgent(agent.agent_id);
      })
      .bindContextMenu(
        () => {
          this.renderSessionContextMenu(agent);
        },
        ResponseType.LongPress
      )
    }
    .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM, top: 1, bottom: 1 })
    .swipeAction({
      end: {
        builder: () => {
          this.renderSwipeArchiveButton(agent.agent_id);
        }
      }
    })
  }

  @Builder
  private renderProjectHeaderRow(group: SidebarProjectGroup) {
    ListItem() {
      Row({ space: 6 }) {
        SymbolGlyph(
          this.normalizedQuery.length === 0 && this.store.isProjectCollapsed(group.projectId)
            ? $r('sys.symbol.chevron_right')
            : $r('sys.symbol.chevron_down')
        )
          .fontSize(12)
          .fontColor([this.store.palette.fgMuted])

        SymbolGlyph($r('sys.symbol.folder'))
          .fontSize(16)
          .fontColor([
            this.store.isNewSessionDraft && cwdPathsEquivalent(group.primaryDir, this.store.activeCwd)
              ? this.store.palette.accentPrimary
              : this.store.palette.iconSecondary
          ])

        Text(group.name)
          .attributeModifier(
            orbitTextSubhead(
              this.store.palette.fgPrimary,
              this.store.isNewSessionDraft && cwdPathsEquivalent(group.primaryDir, this.store.activeCwd)
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
          this.store.startNewSessionDraft(group.primaryDir);
        })
      }
      .width('100%')
      .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
      .padding({ left: 8, right: 4 })
      .backgroundColor(
        this.store.isNewSessionDraft && cwdPathsEquivalent(group.primaryDir, this.store.activeCwd)
          ? this.store.palette.subtleSurface
          : Color.Transparent
      )
      .borderRadius(OrbitTokens.RADIUS_MD)
      .alignItems(VerticalAlign.Center)
      .onClick(() => {
        dismissHarmonyKeyboard(this.getUIContext());
        this.store.toggleProjectCollapsed(group.projectId);
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
        Search({ value: this.searchQuery, placeholder: 'Search' })
          .layoutWeight(1)
          .height(OrbitTokens.HIT_COMPACT)
          .backgroundColor(this.store.palette.subtleSurface)
          .placeholderColor(this.store.palette.fgMuted)
          .fontColor(this.store.palette.fgPrimary)
          .textFont({ size: OrbitTokens.FONT_SUBHEAD_RES })
          .placeholderFont({ size: OrbitTokens.FONT_SUBHEAD_RES })
          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
          .onChange((val: string) => {
            this.searchQuery = val;
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
              (agent: AgentCardDto) => {
                this.renderConversationRow(agent, true, 12)
              },
              (agent: AgentCardDto) => `attn_${agent.agent_id}`
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
              (agent: AgentCardDto) => {
                this.renderConversationRow(agent, true, 12)
              },
              (agent: AgentCardDto) => `pin_${agent.agent_id}`
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
                      this.searchQuery = '';
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
              (group: SidebarProjectGroup) => {
                ListItemGroup() {
                  this.renderProjectHeaderRow(group)

                  if (
                    this.normalizedQuery.length > 0 ||
                    !this.store.isProjectCollapsed(group.projectId)
                  ) {
                    ForEach(
                      this.visibleThreadsForGroup(group),
                      (agent: AgentCardDto) => {
                        this.renderConversationRow(agent, false, 20)
                      },
                      (agent: AgentCardDto) => `t_${group.projectId}_${agent.agent_id}`
                    )

                    if (
                      this.normalizedQuery.length === 0 &&
                      group.threads.length > MAX_VISIBLE_THREADS_PER_PROJECT
                    ) {
                      ListItem() {
                        Button(
                          this.isProjectMoreExpanded(group.projectId)
                            ? 'Show less'
                            : `Show ${group.threads.length - MAX_VISIBLE_THREADS_PER_PROJECT} more…`,
                          { type: ButtonType.Normal, stateEffect: true }
                        )
                          .attributeModifier(orbitButtonSubhead(this.store.palette.fgMuted, Color.Transparent))
                          .width('100%')
                          .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                          .padding({ left: 46, right: OrbitTokens.SPACE_MD })
                          .onClick(() => {
                            this.toggleProjectMoreExpanded(group.projectId);
                          })
                      }
                      .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
                    }
                  }
                }
              },
              (group: SidebarProjectGroup) => `proj_${group.projectId}`
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
