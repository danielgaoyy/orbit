import { SymbolGlyphModifier } from '@kit.ArkUI';
import {
  HostCardDto,
  SidebarConversationDto,
  SidebarProjectGroupDto,
  basenameOfPath,
  cwdPathsEquivalent,
  formatProviderLabel,
  parseRfc3339Ms
} from '../../model/OrbitTypes';
import { orbitRuntime } from '../../model/OrbitRuntime';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { OrbitSidebarRow } from '../../model/stores/OrbitFleetStore';
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
  OrbitAgentListItem,
  mapAgentCardToListStatus,
  resolveAgentEffortDisplay,
  resolveAgentFallbackLabel,
  resolveCompactAgentModelLabel
} from '../common/OrbitAgentListItem';
import { resolveConnectionStateResource } from './TopBarDisplayModel';
import {
  compactTimeSpecToResource,
  resolveRelativeTimeSpec
} from '../personal-agent/PersonalAgentBriefingModel';

@ComponentV2
export struct FleetSidebar {
  private readonly store: OrbitHarmonyStore = orbitRuntime();

  @Computed
  get normalizedQuery(): string {
    return this.store.sidebarSearchQuery.trim().toLowerCase();
  }

  @Computed
  get selectedHostCard(): HostCardDto | null {
    return resolveSelectedHost(this.store);
  }

  private hasCustomSortOrFilter(): boolean {
    return (
      this.store.sidebarProjectSort !== 'recent' ||
      this.store.sidebarThreadFilter !== 'all'
    );
  }

  private hostConnectionResource(host: HostCardDto): Resource {
    return resolveConnectionStateResource(host.connection_state, host.via_ssh_tunnel);
  }

  private resolveConversationRelativeTime(rfc3339: string): ResourceStr {
    const ms = parseRfc3339Ms(rfc3339);
    if (ms <= 0) {
      return '';
    }
    return compactTimeSpecToResource(resolveRelativeTimeSpec(ms, Date.now()));
  }

  private paNeedsYouCount(): number {
    const briefing = this.store.briefing;
    return briefing !== null ? briefing.counts.needs_you : 0;
  }

  private isPersonalAgentActive(): boolean {
    return this.store.showPersonalAgentSetup || this.store.isCurrentSessionPersonalAgent();
  }

  private resolveConversationTitle(conv: SidebarConversationDto): string {
    const title = conv.agent.title.trim();
    return title.length > 0 ? title : formatProviderLabel(conv.agent.provider);
  }

  private resolveConversationModel(conv: SidebarConversationDto): string {
    const detail = this.store.fleetStore.getCachedHostDetail(conv.host_id);
    const catalogs = detail !== null ? detail.provider_catalogs : [];
    return resolveCompactAgentModelLabel(conv.agent, catalogs);
  }

  @Builder
  private renderFilterSortMenu() {
    Menu() {
      MenuItemGroup({ header: $r('app.string.sidebar_sortBy') }) {
        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.clock'))
            .fontColor([this.store.palette.iconPrimary]),
          content: $r('app.string.sidebar_sortRecent'),
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
          content: $r('app.string.sidebar_sortName'),
          symbolEndIcon: this.store.sidebarProjectSort === 'name'
            ? new SymbolGlyphModifier($r('sys.symbol.checkmark')).fontColor([this.store.palette.accentPrimary])
            : undefined
        })
          .onClick(() => {
            this.store.setSidebarProjectSort('name');
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.line_3_horizontal'))
            .fontColor([this.store.palette.iconPrimary]),
          content: $r('app.string.sidebar_sortCustom'),
          symbolEndIcon: this.store.sidebarProjectSort === 'manual'
            ? new SymbolGlyphModifier($r('sys.symbol.checkmark')).fontColor([this.store.palette.accentPrimary])
            : undefined
        })
          .onClick(() => {
            this.store.setSidebarProjectSort('manual');
          })
      }

      MenuItemGroup({ header: $r('app.string.sidebar_show') }) {
        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.message'))
            .fontColor([this.store.palette.iconPrimary]),
          content: $r('app.string.sidebar_filterAll'),
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
          content: $r('app.string.sidebar_filterActiveUnread'),
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
      MenuItemGroup({ header: $r('app.string.briefing_hostsTitle') }) {
        ForEach(
          this.store.hosts,
          (host: HostCardDto) => {
            MenuItem({
              symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.dot_circle'))
                .fontColor([resolveHostStatusColor(host, this.store)]),
              content: host.display_name,
              labelInfo: this.hostConnectionResource(host),
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
          (host: HostCardDto): string => `host_menu_${host.host_id}`
        )
      }

      MenuItemGroup() {
        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.qrcode'))
            .fontColor([this.store.palette.iconPrimary]),
          content: $r('app.string.sidebar_pairNewHost')
        })
          .onClick(() => {
            this.store.openHostSettings();
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.bolt'))
            .fontColor([this.store.palette.iconPrimary]),
          content: $r('app.string.sidebar_connectLocalHost')
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
          content: $r('app.string.sidebar_newSessionHere')
        })
          .onClick(() => {
            this.store.startNewSessionDraft(group.primary_cwd);
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.folder'))
            .fontColor([this.store.palette.iconPrimary]),
          content: $r('app.string.sidebar_selectWorkspace')
        })
          .onClick(() => {
            this.store.startNewSessionDraft(group.primary_cwd);
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.doc_text'))
            .fontColor([this.store.palette.iconPrimary]),
          content: $r('app.string.nav_file_explorer')
        })
          .onClick(() => {
            this.store.openFileExplorer(undefined, group.primary_cwd, group.host_id);
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.arrow_left_arrow_right'))
            .fontColor([this.store.palette.iconPrimary]),
          content: $r('app.string.nav_git_review')
        })
          .onClick(() => {
            this.store.inspectGitDiffForCwd(group.primary_cwd, group.host_id);
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.code_square'))
            .fontColor([this.store.palette.iconPrimary]),
          content: $r('app.string.nav_terminal')
        })
          .onClick(() => {
            this.store.openPtyTerminalInCwd(group.primary_cwd, group.host_id);
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier(
            (group.pinned_project_id ?? '').length > 0 ? $r('sys.symbol.pin_slash') : $r('sys.symbol.pin')
          ).fontColor([this.store.palette.iconPrimary]),
          content: (group.pinned_project_id ?? '').length > 0
            ? $r('app.string.sidebar_unpinProject')
            : $r('app.string.sidebar_pinProject')
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
          content: $r('app.string.sidebar_copyPath')
        })
          .onClick(() => {
            this.store.copyToClipboard(group.primary_cwd, '');
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
          content: conv.is_pinned ? $r('app.string.sidebar_unpin') : $r('app.string.sidebar_pin')
        })
          .onClick(() => {
            this.store.togglePinThread(conv.host_id, conv.agent.agent_id);
          })

        if (conv.is_unread) {
          MenuItem({
            symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.checkmark_circle'))
              .fontColor([this.store.palette.iconPrimary]),
            content: $r('app.string.sidebar_markRead')
          })
            .onClick(() => {
              this.store.markAgentRead(conv.host_id, conv.agent.agent_id);
            })
        } else {
          MenuItem({
            symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.dot_circle'))
              .fontColor([this.store.palette.iconPrimary]),
            content: $r('app.string.sidebar_markUnread')
          })
            .onClick(() => {
              this.store.markAgentUnread(conv.host_id, conv.agent.agent_id);
            })
        }

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.plus'))
            .fontColor([this.store.palette.iconPrimary]),
          content: $r('app.string.sidebar_newSessionHere')
        })
          .onClick(() => {
            this.store.startNewSessionDraft(conv.agent.cwd);
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.arrow_left_arrow_right'))
            .fontColor([this.store.palette.iconPrimary]),
          content: $r('app.string.nav_git_review')
        })
          .onClick(() => {
            this.store.inspectGitDiffForCwd(conv.agent.cwd, conv.host_id);
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.code_square'))
            .fontColor([this.store.palette.iconPrimary]),
          content: $r('app.string.nav_terminal')
        })
          .onClick(() => {
            this.store.openPtyTerminalInCwd(conv.agent.cwd, conv.host_id);
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.square_and_square'))
            .fontColor([this.store.palette.iconPrimary]),
          content: $r('app.string.sidebar_copyPath')
        })
          .onClick(() => {
            this.store.copyToClipboard(conv.agent.cwd, '');
          })
      }

      MenuItemGroup() {
        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.archivebox'))
            .fontColor([this.store.palette.statusDanger]),
          content: $r('app.string.common_archive')
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
      .accessibilityText($r('app.string.sidebar_archiveConversation'))
      .onClick(() => {
        this.store.archiveSession(agentId);
      })
    }
    .height('100%')
    .padding({ left: OrbitTokens.SPACE_XS })
  }

  @Builder
  private renderPersonalAgentEntry() {
    Column({ space: 2 }) {
      // Personal Agent 30vp primary navigation row
      Row({ space: OrbitTokens.SPACE_SM }) {
        Image($r('app.media.orbit_agent_bot'))
          .width(15)
          .height(15)
          .objectFit(ImageFit.Contain)
          .fillColor($r('app.color.icon_primary'))
          .draggable(false)
          .flexShrink(0)

        Text($r('app.string.sidebar_personalRouter'))
          .fontSize(OrbitTokens.FONT_SUBHEAD)
          .fontWeight(this.isPersonalAgentActive() ? FontWeight.Medium : FontWeight.Normal)
          .fontColor(this.store.palette.fgPrimary)
          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .layoutWeight(1)

        if (this.paNeedsYouCount() > 0) {
          Text($r('app.string.sidebar_personalAgentNeedsYou', this.paNeedsYouCount()))
            .fontSize(OrbitTokens.FONT_META)
            .fontWeight(FontWeight.Medium)
            .fontColor(this.store.palette.statusWarning)
            .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
            .maxLines(1)
            .padding({ left: 6, right: 6, top: 2, bottom: 2 })
            .borderRadius(OrbitTokens.RADIUS_SM)
            .backgroundColor(this.store.palette.statusWarningSoft)
            .flexShrink(0)
        }
      }
      .width('100%')
      .constraintSize({ minHeight: 30 })
      .padding({ left: 10, right: 10, top: 5, bottom: 5 })
      .borderRadius(OrbitTokens.RADIUS_MD)
      .backgroundColor(
        this.isPersonalAgentActive()
          ? this.store.palette.subtleSurface
          : Color.Transparent
      )
      .alignItems(VerticalAlign.Center)
      .accessibilityText($r('app.string.sidebar_personalRouter'))
      .onClick(() => {
        dismissHarmonyKeyboard(this.getUIContext());
        this.store.openPersonalAgentHome();
      })

      // Fleet-wide search shortcut
      Row({ space: OrbitTokens.SPACE_SM }) {
        SymbolGlyph($r('sys.symbol.magnifyingglass'))
          .fontSize(14)
          .fontColor([this.store.palette.iconSecondary])
          .flexShrink(0)

        Text($r('app.string.nav_search_fleet'))
          .fontSize(OrbitTokens.FONT_SUBHEAD)
          .fontWeight(FontWeight.Normal)
          .fontColor(this.store.palette.fgSecondary)
          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .layoutWeight(1)
      }
      .width('100%')
      .constraintSize({ minHeight: 30 })
      .padding({ left: 10, right: 10, top: 5, bottom: 5 })
      .borderRadius(OrbitTokens.RADIUS_MD)
      .alignItems(VerticalAlign.Center)
      .accessibilityText($r('app.string.nav_search_fleet'))
      .onClick(() => {
        dismissHarmonyKeyboard(this.getUIContext());
        this.store.openGlobalSearch();
      })
    }
    .width('100%')
    .padding({
      left: OrbitTokens.SPACE_SM,
      right: OrbitTokens.SPACE_SM,
      bottom: OrbitTokens.SPACE_XS
    })
  }

  @Builder
  private renderProjectsSectionHeader() {
    Row({ space: 4 }) {
      Text($r('app.string.sidebar_projects'))
        .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Medium))
        .maxLines(1)
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
      .accessibilityText($r('app.string.sidebar_sortAndFilterAria'))
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
      .accessibilityText($r('app.string.sidebar_browseDirectoryAria'))
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

  @Builder
  private renderProjectHeaderRow(group: SidebarProjectGroupDto, isCollapsed: boolean) {
    Row({ space: 6 }) {
      SymbolGlyph(
        isCollapsed ? $r('sys.symbol.chevron_right') : $r('sys.symbol.chevron_down')
      )
        .fontSize(12)
        .fontColor([this.store.palette.fgMuted])
        .flexShrink(0)

      SymbolGlyph($r('sys.symbol.folder'))
        .fontSize(15)
        .fontColor([
          this.store.isNewSessionDraft && cwdPathsEquivalent(group.primary_cwd, this.store.activeCwd)
            ? this.store.palette.accentPrimary
            : this.store.palette.iconSecondary
        ])
        .flexShrink(0)

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
          .fontSize(15)
          .renderingStrategy(SymbolRenderingStrategy.SINGLE)
          .fontColor([this.store.palette.iconSecondary])
      }
      .width(OrbitTokens.HIT_COMPACT)
      .height(OrbitTokens.HIT_COMPACT)
      .padding(0)
      .backgroundColor(Color.Transparent)
      .accessibilityText($r('app.string.sidebar_newConversationInTarget', group.name))
      .onClick(() => {
        dismissHarmonyKeyboard(this.getUIContext());
        this.store.startNewSessionDraft(group.primary_cwd);
      })
    }
    .width('100%')
    .constraintSize({ minHeight: 40 })
    .padding({ left: 8, right: 4, top: 4, bottom: 4 })
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

  @Builder
  private renderEmptyProjectsRow() {
    Column({ space: OrbitTokens.SPACE_SM }) {
      Text(
        this.normalizedQuery.length > 0
          ? $r('app.string.sidebar_noMatchingThreads')
          : this.store.sidebarThreadFilter === 'active_unread'
            ? $r('app.string.sidebar_noActiveOrUnread')
            : $r('app.string.sidebar_noProjectsYet')
      )
        .attributeModifier(orbitTextSubhead(this.store.palette.fgMuted, FontWeight.Normal))

      if (this.normalizedQuery.length > 0) {
        Button($r('app.string.sidebar_clearThreadFilter'), { type: ButtonType.Normal, stateEffect: true })
          .attributeModifier(orbitButtonSubhead(this.store.palette.accentPrimary, Color.Transparent))
          .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
          .padding({ left: 0, right: OrbitTokens.SPACE_SM })
          .onClick(() => {
            this.store.setSidebarSearchQuery('');
          })
      } else if (this.store.sidebarThreadFilter !== 'all') {
        Button($r('app.string.sidebar_showAllConversations'), { type: ButtonType.Normal, stateEffect: true })
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

  @Builder
  private renderHostSwitcherPillBody() {
    Row()
      .width(8)
      .height(8)
      .borderRadius(4)
      .backgroundColor(resolveHostStatusColor(this.selectedHostCard, this.store))
      .flexShrink(0)

    Text(this.selectedHostCard !== null ? this.selectedHostCard.display_name : $r('app.string.sidebar_noHostShort'))
      .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold))
      .maxLines(1)
      .textOverflow({ overflow: TextOverflow.Ellipsis })
      .flexShrink(1)

    if (this.selectedHostCard !== null) {
      Row({ space: 4 }) {
        Text(this.hostConnectionResource(this.selectedHostCard))
          .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
          .maxLines(1)
          .flexShrink(0)
        if (this.selectedHostCard.last_ping_ms !== null) {
          Text(`· ${this.selectedHostCard.last_ping_ms}ms`)
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
            .flexShrink(1)
        }
      }
      .layoutWeight(1)
      .alignItems(VerticalAlign.Center)
    } else {
      Text($r('app.string.sidebar_tapToPairHost'))
        .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
        .layoutWeight(1)
    }

    if (this.store.otherHostsPendingApprovals() > 0) {
      Text(`${this.store.otherHostsPendingApprovals()}`)
        .attributeModifier(orbitTextMeta(this.store.palette.statusWarning, FontWeight.Bold))
        .padding({ left: 6, right: 6, top: 2, bottom: 2 })
        .borderRadius(OrbitTokens.RADIUS_MD)
        .backgroundColor(this.store.palette.statusWarningSoft)
        .flexShrink(0)
    }

    SymbolGlyph($r('sys.symbol.chevron_down'))
      .fontSize(12)
      .fontColor([this.store.palette.fgMuted])
      .flexShrink(0)
  }

  @Builder
  private renderVirtualizedRow(row: OrbitSidebarRow) {
    if (row.kind === 'hdr_attention') {
      ListItem() {
        Text($r('app.string.sidebar_needsAttention'))
          .attributeModifier(orbitTextMeta(this.store.palette.statusWarning, FontWeight.Medium))
          .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG, top: 6, bottom: 4 })
      }
    } else if (row.kind === 'hdr_pinned') {
      ListItem() {
        Text($r('app.string.sidebar_pinned'))
          .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Medium))
          .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG, top: 10, bottom: 4 })
      }
    } else if (row.kind === 'hdr_projects') {
      ListItem() {
        this.renderProjectsSectionHeader()
      }
    } else if (row.kind === 'hdr_empty_projects') {
      ListItem() {
        this.renderEmptyProjectsRow()
      }
    } else if (row.kind === 'proj' && row.project !== null) {
      ListItem() {
        this.renderProjectHeaderRow(row.project, row.isProjectCollapsed)
      }
      .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM, top: 1, bottom: 1 })
    } else if (row.kind === 'proj_more' && row.project !== null) {
      ListItem() {
        Button(
          row.isMoreExpanded
            ? $r('app.string.common_showLess')
            : $r('app.string.common_showMore', row.hiddenCount),
          { type: ButtonType.Normal, stateEffect: true }
        )
          .attributeModifier(orbitButtonSubhead(this.store.palette.fgMuted, Color.Transparent))
          .width('100%')
          .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
          .padding({ left: 36, right: OrbitTokens.SPACE_MD })
          .onClick(() => {
            if (row.project !== null) {
              this.store.toggleProjectMoreExpanded(row.project.id);
            }
          })
      }
      .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
    } else if (row.kind === 'conv' && row.conv !== null) {
      ListItem() {
        Row() {
          OrbitAgentListItem({
            variant: 'row',
            leadingMode: 'signal',
            title: this.resolveConversationTitle(row.conv),
            status: mapAgentCardToListStatus(row.conv.agent),
            avatarSeedKey: row.conv.agent.agent_id,
            provider: row.conv.agent.provider,
            modelLabel: this.resolveConversationModel(row.conv),
            effortLabel: resolveAgentEffortDisplay(row.conv.agent),
            agentLabel: resolveAgentFallbackLabel(row.conv.agent),
            secondaryMeta: row.showWorkspaceHint ? basenameOfPath(row.conv.agent.cwd) : '',
            titleTrailingText: this.resolveConversationRelativeTime(row.conv.agent.updated_at_rfc3339),
            isUnread: row.conv.is_unread,
            hasArmedTimers: row.conv.agent.has_armed_timers === true,
            isWorktree: row.conv.is_worktree,
            isSelected: !this.store.isNewSessionDraft &&
              !this.store.showPersonalAgentSetup &&
              this.store.selectedHostId === row.conv.host_id &&
              this.store.selectedAgentId === row.conv.agent.agent_id,
            onItemClick: () => {
              if (row.conv !== null) {
                dismissHarmonyKeyboard(this.getUIContext());
                this.store.selectAgentOnHost(row.conv.host_id, row.conv.agent.agent_id, true);
              }
            }
          })
        }
        .width('100%')
        .padding({ left: row.indentLeft })
        .bindContextMenu(
          () => {
            if (row.conv !== null) {
              this.renderSessionContextMenu(row.conv);
            }
          },
          ResponseType.LongPress
        )
      }
      .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM, top: 1, bottom: 1 })
      .swipeAction({
        end: {
          builder: () => {
            if (row.conv !== null) {
              this.renderSwipeArchiveButton(row.conv.agent.agent_id);
            }
          }
        }
      })
    }
  }

  build() {
    Column() {
      // 1. Search + New Conversation Row
      Row({ space: OrbitTokens.SPACE_SM }) {
        Search({
          value: this.store.sidebarSearchQuery,
          placeholder: $r('app.string.sidebar_search')
        })
          .layoutWeight(1)
          .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
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
        .accessibilityText($r('app.string.sidebar_newConversation'))
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

      // 2. Personal Agent Entry + Virtualized Flat List
      if (this.store.hosts.length === 0) {
        Column({ space: OrbitTokens.SPACE_MD }) {
          Column({ space: OrbitTokens.SPACE_SM }) {
            Text($r('app.string.sidebar_noHostsTitle'))
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold))
            Text($r('app.string.sidebar_noHostsBody'))
              .attributeModifier(orbitTextSubhead(this.store.palette.fgSecondary, FontWeight.Normal))

            Button($r('app.string.sidebar_connectLocalDaemon'))
              .attributeModifier(
                orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
              )
              .width('100%')
              .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
              .borderRadius(OrbitTokens.RADIUS_MD)
              .onClick(() => {
                this.store.discoverAndPairLocalDaemon();
              })

            Button($r('app.string.sidebar_pairRemoteHost'))
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
        this.renderPersonalAgentEntry()

        List() {
          Repeat<OrbitSidebarRow>(this.store.fleetStore.sidebarRows)
            .each((ri: RepeatItem<OrbitSidebarRow>) => {
              this.renderVirtualizedRow(ri.item)
            })
            .key((row: OrbitSidebarRow): string => row.key)
            .virtualScroll({ totalCount: this.store.fleetStore.sidebarRows.length })
        }
        .cachedCount(5)
        .width('100%')
        .layoutWeight(1)
        .scrollBar(BarState.Auto)
        .edgeEffect(EdgeEffect.Spring)
        .padding({ bottom: OrbitTokens.SPACE_SM })
        .onScrollStart(() => {
          dismissHarmonyKeyboard(this.getUIContext());
        })
      }

      // 3. Thumb-Zone Bottom Bar: [ Host Switcher Capsule ] [ Automations ] [ Archived ] [ Settings ]
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
          .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
          .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD, top: 6, bottom: 6 })
          .backgroundColor(this.store.palette.subtleSurface)
          .accessibilityText($r('app.string.sidebar_switchHostAria'))
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
          .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
          .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD, top: 6, bottom: 6 })
          .backgroundColor(this.store.palette.subtleSurface)
          .accessibilityText($r('app.string.sidebar_pairHostAria'))
          .onClick(() => {
            this.store.openHostSettings();
          })
        }

        if (this.store.hosts.length > 0) {
          Button({ type: ButtonType.Circle, stateEffect: true }) {
            SymbolGlyph($r('sys.symbol.clock'))
              .fontSize(18)
              .renderingStrategy(SymbolRenderingStrategy.SINGLE)
              .fontColor([this.store.palette.iconPrimary])
          }
          .width(OrbitTokens.HIT_COMPACT)
          .height(OrbitTokens.HIT_COMPACT)
          .padding(0)
          .backgroundColor(this.store.palette.subtleSurface)
          .accessibilityText($r('app.string.sidebar_automations'))
          .onClick(() => {
            dismissHarmonyKeyboard(this.getUIContext());
            this.store.openAutomationsSheet();
          })

          Button({ type: ButtonType.Circle, stateEffect: true }) {
            SymbolGlyph($r('sys.symbol.archivebox'))
              .fontSize(18)
              .renderingStrategy(SymbolRenderingStrategy.SINGLE)
              .fontColor([this.store.palette.iconPrimary])
          }
          .width(OrbitTokens.HIT_COMPACT)
          .height(OrbitTokens.HIT_COMPACT)
          .padding(0)
          .backgroundColor(this.store.palette.subtleSurface)
          .accessibilityText($r('app.string.sidebar_archived'))
          .onClick(() => {
            dismissHarmonyKeyboard(this.getUIContext());
            this.store.openArchivedSessions();
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
        .accessibilityText($r('app.string.sidebar_settingsAndHostsAria'))
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
