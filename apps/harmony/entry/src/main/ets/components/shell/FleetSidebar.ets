import {
  AgentCardDto,
  ClientProjectDto,
  DirectoryEntryDto,
  DirectoryListingDto,
  DiscoveredProjectDto,
  HostCardDto,
  basenameOfPath,
  cwdPathsEquivalent,
  formatAgentStatusLabel,
  formatConnectionStateLabel
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitButtonSubhead,
  orbitInputSubhead,
  orbitTextBody,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead
} from '../../theme/OrbitTokens';

interface SidebarFilterChip {
  key: string;
  label: string;
}

const SIDEBAR_FILTERS: SidebarFilterChip[] = [
  { key: 'all', label: 'All' },
  { key: 'active', label: 'Active' },
  { key: 'approvals', label: 'Approvals' }
];

@ComponentV2
export struct FleetSidebar {
  @Local searchQuery: string = '';
  @Local activeFilter: string = 'all';

  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  private hostStatusDotColor(host: HostCardDto): string {
    if (host.pending_approval_count > 0) {
      return this.store.palette.statusWarning;
    }
    if (host.connection_state === 'online_direct') {
      return this.store.palette.statusDirect;
    }
    if (host.connection_state === 'online_relay') {
      return this.store.palette.statusRelay;
    }
    if (
      host.connection_state === 'connecting_direct' ||
      host.connection_state === 'connecting_relay'
    ) {
      return this.store.palette.accentPrimary;
    }
    return this.store.palette.fgMuted;
  }

  private normalizedQuery(): string {
    return this.searchQuery.trim().toLowerCase();
  }

  private matchesAgentSearch(agent: AgentCardDto): boolean {
    const q = this.normalizedQuery();
    if (q.length === 0) {
      return true;
    }
    const title = (agent.title.trim().length > 0 ? agent.title : 'Untitled Session').toLowerCase();
    const cwd = agent.cwd.toLowerCase();
    const cwdBase = basenameOfPath(agent.cwd).toLowerCase();
    const provider = agent.provider.toLowerCase();
    const providerLabel = agent.provider_label.toLowerCase();
    return (
      title.indexOf(q) >= 0 ||
      cwd.indexOf(q) >= 0 ||
      cwdBase.indexOf(q) >= 0 ||
      provider.indexOf(q) >= 0 ||
      providerLabel.indexOf(q) >= 0
    );
  }

  private matchesProjectSearch(proj: ClientProjectDto): boolean {
    const q = this.normalizedQuery();
    if (q.length === 0) {
      return true;
    }
    if (proj.name.toLowerCase().indexOf(q) >= 0) {
      return true;
    }
    for (let i = 0; i < proj.directories.length; i++) {
      const dir = proj.directories[i];
      if (dir.toLowerCase().indexOf(q) >= 0) {
        return true;
      }
      const liveBranch = this.store.resolveWorkspaceBranch(dir).toLowerCase();
      if (liveBranch.length > 0 && liveBranch.indexOf(q) >= 0) {
        return true;
      }
    }
    return false;
  }

  private matchesRepoSearch(repo: DiscoveredProjectDto): boolean {
    const q = this.normalizedQuery();
    if (q.length === 0) {
      return true;
    }
    const name = repo.name.toLowerCase();
    const path = repo.path.toLowerCase();
    const liveBranch = this.store.resolveWorkspaceBranch(repo.path).toLowerCase();
    const staticBranch = (repo.git_branch ?? '').toLowerCase();
    return (
      name.indexOf(q) >= 0 ||
      path.indexOf(q) >= 0 ||
      liveBranch.indexOf(q) >= 0 ||
      staticBranch.indexOf(q) >= 0
    );
  }

  private pinnedProjectsForHost(): ClientProjectDto[] {
    const out: ClientProjectDto[] = [];
    for (let i = 0; i < this.store.projects.length; i++) {
      const p = this.store.projects[i];
      if (p.host_id === this.store.selectedHostId && this.matchesProjectSearch(p)) {
        out.push(p);
      }
    }
    return out;
  }

  private discoveredRepos(): DiscoveredProjectDto[] {
    const detail = this.store.currentHostDetail;
    if (detail === null) {
      return [];
    }
    const matched: DiscoveredProjectDto[] = [];
    for (let i = 0; i < detail.discovered_projects.length; i++) {
      const repo = detail.discovered_projects[i];
      if (this.matchesRepoSearch(repo)) {
        matched.push(repo);
        if (matched.length >= 8) {
          break;
        }
      }
    }
    return matched;
  }

  private directoryListing(): DirectoryListingDto | null {
    const detail = this.store.currentHostDetail;
    return detail !== null ? detail.directory_listing : null;
  }

  private hasUnfilteredApprovals(): boolean {
    const detail = this.store.currentHostDetail;
    if (detail === null) {
      return false;
    }
    for (let i = 0; i < detail.agents.length; i++) {
      const a = detail.agents[i];
      if (
        a.status.state !== 'closed' &&
        (a.is_awaiting_permission || a.status.state === 'awaiting_permission')
      ) {
        return true;
      }
    }
    return false;
  }

  private approvalAgents(): AgentCardDto[] {
    const detail = this.store.currentHostDetail;
    if (detail === null) {
      return [];
    }
    const out: AgentCardDto[] = [];
    for (let i = 0; i < detail.agents.length; i++) {
      const a = detail.agents[i];
      if (
        a.status.state !== 'closed' &&
        (a.is_awaiting_permission || a.status.state === 'awaiting_permission') &&
        this.matchesAgentSearch(a)
      ) {
        out.push(a);
      }
    }
    return out;
  }

  private activeAgents(): AgentCardDto[] {
    const detail = this.store.currentHostDetail;
    if (detail === null) {
      return [];
    }
    const out: AgentCardDto[] = [];
    for (let i = 0; i < detail.agents.length; i++) {
      const a = detail.agents[i];
      if (
        a.status.state !== 'closed' &&
        !a.is_awaiting_permission &&
        a.status.state !== 'awaiting_permission' &&
        a.is_running &&
        this.matchesAgentSearch(a)
      ) {
        out.push(a);
      }
    }
    return out;
  }

  private recentAgents(): AgentCardDto[] {
    const detail = this.store.currentHostDetail;
    if (detail === null) {
      return [];
    }
    const out: AgentCardDto[] = [];
    for (let i = 0; i < detail.agents.length; i++) {
      const a = detail.agents[i];
      if (
        a.status.state !== 'closed' &&
        !a.is_awaiting_permission &&
        a.status.state !== 'awaiting_permission' &&
        !a.is_running &&
        this.matchesAgentSearch(a)
      ) {
        out.push(a);
      }
    }
    return out;
  }

  @Builder
  private renderSwipeArchiveButton(agentId: string) {
    Row() {
      Button('Archive')
        .attributeModifier(
          orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.statusDanger)
        )
        .height('100%')
        .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
        .borderRadius(OrbitTokens.CARD_RADIUS)
        .onClick(() => {
          this.store.archiveAgent(agentId);
        })
    }
    .height('100%')
    .padding({ left: OrbitTokens.SPACE_XS })
  }

  @Builder
  private renderSessionContextMenu(agent: AgentCardDto) {
    Menu() {
      MenuItem({ content: 'New Session in Workspace' })
        .onClick(() => {
          this.store.startNewSessionDraft(agent.cwd);
        })
      MenuItem({ content: 'Open Terminal Here' })
        .onClick(() => {
          this.store.openPtyTerminalInCwd(agent.cwd);
        })
      MenuItem({ content: 'Copy Workspace Path' })
        .onClick(() => {
          this.store.copyToClipboard(agent.cwd, 'Copied workspace path');
        })
      MenuItem({ content: 'Archive Session' })
        .contentFontColor(this.store.palette.statusDanger)
        .onClick(() => {
          this.store.archiveAgent(agent.agent_id);
        })
    }
  }

  @Builder
  private renderWorkspaceContextMenu(cwd: string) {
    Menu() {
      MenuItem({ content: 'New Session Here' })
        .onClick(() => {
          this.store.startNewSessionDraft(cwd);
        })
      MenuItem({
        content: this.store.findPinnedProjectIdForCwd(cwd).length > 0
          ? 'Unpin Workspace'
          : 'Pin Workspace'
      })
        .onClick(() => {
          if (this.store.findPinnedProjectIdForCwd(cwd).length > 0) {
            this.store.unpinWorkspaceByCwd(cwd);
          } else {
            this.store.pinDirectoryAsProject(cwd);
          }
        })
      MenuItem({ content: 'Inspect Git Diff' })
        .onClick(() => {
          this.store.inspectGitDiffForCwd(cwd);
        })
      MenuItem({ content: 'Open Terminal Here' })
        .onClick(() => {
          this.store.openPtyTerminalInCwd(cwd);
        })
      MenuItem({ content: 'Copy Path' })
        .onClick(() => {
          this.store.copyToClipboard(cwd, 'Copied workspace path');
        })
    }
  }

  @Builder
  private renderAgentRow(agent: AgentCardDto) {
    ListItem() {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Row()
          .width(OrbitTokens.DOT_SIZE)
          .height(OrbitTokens.DOT_SIZE)
          .borderRadius(3)
          .backgroundColor(
            agent.is_awaiting_permission || agent.status.state === 'awaiting_permission'
              ? this.store.palette.statusWarning
              : agent.is_running
                ? this.store.palette.accentPrimary
                : agent.status.state === 'error'
                  ? this.store.palette.statusDanger
                  : this.store.palette.statusDirect
          )
          .margin({ top: 7 })

        Column({ space: 2 }) {
          Text(agent.title.trim().length > 0 ? agent.title : 'Untitled Session')
            .attributeModifier(orbitTextBody(this.store.palette.fgPrimary, FontWeight.Medium))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })

          Text(
            `${basenameOfPath(agent.cwd)} · ${agent.provider_label} · ${formatAgentStatusLabel(agent)}`
          )
            .attributeModifier(
              orbitTextMeta(
                agent.is_awaiting_permission
                  ? this.store.palette.statusWarning
                  : this.store.palette.fgSecondary,
                FontWeight.Normal
              )
            )
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)
      }
      .width('100%')
      .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
      .padding({
        left: OrbitTokens.SPACE_MD,
        right: OrbitTokens.SPACE_MD,
        top: OrbitTokens.SPACE_SM,
        bottom: OrbitTokens.SPACE_SM
      })
      .backgroundColor(
        this.store.selectedAgentId === agent.agent_id
          ? this.store.palette.subtleSurface
          : Color.Transparent
      )
      .borderRadius(OrbitTokens.CARD_RADIUS)
      .alignItems(VerticalAlign.Top)
      .onClick(() => {
        this.store.selectAgent(agent.agent_id);
      })
      .bindContextMenu(
        () => {
          this.renderSessionContextMenu(agent);
        },
        ResponseType.LongPress
      )
    }
    .swipeAction({
      end: {
        builder: () => {
          this.renderSwipeArchiveButton(agent.agent_id);
        }
      }
    })
  }

  @Builder
  private renderDirectoryBrowserPanel() {
    if (this.store.showDirBrowser) {
      Column({ space: OrbitTokens.SPACE_XS }) {
        Row({ space: OrbitTokens.SPACE_XS }) {
          Text(this.directoryListing()?.current_path ?? this.store.activeCwd)
            .attributeModifier(orbitTextMono(this.store.palette.fgPrimary, FontWeight.Medium))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
            .layoutWeight(1)

          Button('Pin')
            .attributeModifier(
              orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.accentPrimarySoft)
            )
            .height(28)
            .borderRadius(OrbitTokens.PILL_RADIUS)
            .onClick(() => {
              const cur = this.directoryListing()?.current_path ?? this.store.activeCwd;
              this.store.pinDirectoryAsProject(cur);
              this.store.showDirBrowser = false;
            })

          Button('Use')
            .attributeModifier(
              orbitButtonMeta(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
            )
            .height(28)
            .borderRadius(OrbitTokens.PILL_RADIUS)
            .onClick(() => {
              const cur = this.directoryListing()?.current_path ?? this.store.activeCwd;
              this.store.startNewSessionDraft(cur);
              this.store.showDirBrowser = false;
            })
        }
        .width('100%')

        if (
          this.directoryListing() !== null &&
          this.directoryListing()?.parent_path !== null
        ) {
          Row({ space: OrbitTokens.SPACE_SM }) {
            Text('..')
              .attributeModifier(orbitTextMono(this.store.palette.accentPrimary, FontWeight.Bold))
            Text('Parent Directory')
              .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
          }
          .width('100%')
          .height(36)
          .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
          .onClick(() => {
            this.store.browseDirectory(this.directoryListing()?.parent_path ?? null);
          })
        }

        if (this.directoryListing() !== null) {
          ForEach(
            (this.directoryListing()?.entries ?? []).slice(0, 14),
            (entry: DirectoryEntryDto) => {
              Row({ space: OrbitTokens.SPACE_SM }) {
                Text(entry.is_git_repo ? '⎇' : '▸')
                  .attributeModifier(
                    orbitTextMono(
                      entry.is_git_repo
                        ? this.store.palette.statusDirect
                        : this.store.palette.fgMuted
                    )
                  )
                Text(entry.name)
                  .attributeModifier(orbitTextMono(this.store.palette.fgPrimary))
                  .maxLines(1)
                  .textOverflow({ overflow: TextOverflow.Ellipsis })
                  .layoutWeight(1)
              }
              .width('100%')
              .height(36)
              .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
              .borderRadius(OrbitTokens.PILL_RADIUS)
              .onClick(() => {
                this.store.browseDirectory(entry.path);
              })
            },
            (entry: DirectoryEntryDto) => entry.path
          )
        }
      }
      .width('100%')
      .padding(OrbitTokens.SPACE_SM)
      .backgroundColor(this.store.palette.code)
      .borderRadius(OrbitTokens.CARD_RADIUS)
      .border({
        width: OrbitTokens.HAIRLINE,
        color: this.store.palette.hairline
      })
    }
  }

  build() {
    Column({ space: OrbitTokens.SPACE_MD }) {
      Column({ space: OrbitTokens.SPACE_SM }) {
        Row() {
          Text('FLEET HOSTS')
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Bold))
          Blank()
          if (this.store.hosts.length > 0) {
            Button('+ Add')
              .attributeModifier(
                orbitButtonMeta(this.store.palette.accentPrimary, Color.Transparent)
              )
              .height(28)
              .onClick(() => {
                this.store.openHostSettings();
              })
          }
        }
        .width('100%')

        if (this.store.hosts.length === 0) {
          Text('No hosts paired yet.')
            .attributeModifier(orbitTextSubhead(this.store.palette.fgMuted, FontWeight.Normal))
        } else {
          Scroll() {
            Row({ space: OrbitTokens.SPACE_SM }) {
              ForEach(
                this.store.hosts,
                (host: HostCardDto) => {
                  Column({ space: 2 }) {
                    Row({ space: OrbitTokens.SPACE_XS }) {
                      Row()
                        .width(OrbitTokens.DOT_SIZE)
                        .height(OrbitTokens.DOT_SIZE)
                        .borderRadius(3)
                        .backgroundColor(this.hostStatusDotColor(host))

                      Text(host.display_name)
                        .attributeModifier(
                          orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold)
                        )
                        .maxLines(1)
                        .textOverflow({ overflow: TextOverflow.Ellipsis })
                    }

                    Row({ space: OrbitTokens.SPACE_XS }) {
                      Text(formatConnectionStateLabel(host.connection_state, host.via_ssh_tunnel))
                        .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                      if (host.last_ping_ms !== null) {
                        Text(`${host.last_ping_ms}ms`)
                          .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                      }
                    }
                  }
                  .alignItems(HorizontalAlign.Start)
                  .constraintSize({ minWidth: 124, maxWidth: 180, minHeight: OrbitTokens.HIT_MIN })
                  .padding({
                    left: OrbitTokens.SPACE_MD,
                    right: OrbitTokens.SPACE_MD,
                    top: OrbitTokens.SPACE_SM,
                    bottom: OrbitTokens.SPACE_SM
                  })
                  .backgroundColor(
                    this.store.selectedHostId === host.host_id
                      ? this.store.palette.elevated
                      : Color.Transparent
                  )
                  .borderRadius(OrbitTokens.CARD_RADIUS)
                  .border({
                    width: OrbitTokens.HAIRLINE,
                    color: this.store.selectedHostId === host.host_id
                      ? this.store.palette.accentPrimary
                      : this.store.palette.hairline
                  })
                  .onClick(() => {
                    this.store.selectHost(host.host_id);
                    this.store.triggerHaptic(8);
                  })
                },
                (host: HostCardDto) => host.host_id
              )
            }
          }
          .scrollable(ScrollDirection.Horizontal)
          .scrollBar(BarState.Off)
          .width('100%')
        }
      }
      .width('100%')
      .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })

      if (this.store.hosts.length === 0) {
        Column({ space: OrbitTokens.SPACE_XS }) {
          Text('Pair a host below to browse workspaces and sessions.')
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
        }
        .width('100%')
        .layoutWeight(1)
        .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
        .alignItems(HorizontalAlign.Start)
      } else {
        // Instant Session & Workspace Search + Filter Chips
        Column({ space: OrbitTokens.SPACE_SM }) {
          Row({ space: OrbitTokens.SPACE_XS }) {
            Text('⌕')
              .attributeModifier(orbitTextSubhead(this.store.palette.fgMuted, FontWeight.Normal))

            TextInput({
              placeholder: 'Search sessions or repos…',
              text: this.searchQuery
            })
              .attributeModifier(
                orbitInputSubhead(
                  this.store.palette.fgPrimary,
                  this.store.palette.fgMuted,
                  Color.Transparent
                )
              )
              .layoutWeight(1)
              .height(36)
              .padding(0)
              .onChange((val: string) => {
                this.searchQuery = val;
              })

            if (this.searchQuery.length > 0) {
              Stack({ alignContent: Alignment.Center }) {
                Text('×')
                  .attributeModifier(
                    orbitTextSubhead(this.store.palette.fgMuted, FontWeight.Bold)
                  )
              }
              .width(OrbitTokens.HIT_MIN)
              .height(OrbitTokens.HIT_MIN)
              .accessibilityText('Clear search query')
              .onClick(() => {
                this.searchQuery = '';
                this.store.triggerHaptic(6);
              })
            }
          }
          .width('100%')
          .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
          .padding({ left: OrbitTokens.SPACE_SM, right: 0 })
          .backgroundColor(this.store.palette.elevated)
          .borderRadius(OrbitTokens.PILL_RADIUS)
          .border({
            width: OrbitTokens.HAIRLINE,
            color: this.store.palette.hairline
          })
          .alignItems(VerticalAlign.Center)

          Row({ space: OrbitTokens.SPACE_XS }) {
            ForEach(
              SIDEBAR_FILTERS,
              (chip: SidebarFilterChip) => {
                Row({ space: OrbitTokens.SPACE_XS }) {
                  if (chip.key === 'approvals' && this.hasUnfilteredApprovals()) {
                    Row()
                      .width(OrbitTokens.DOT_SIZE)
                      .height(OrbitTokens.DOT_SIZE)
                      .borderRadius(3)
                      .backgroundColor(this.store.palette.statusWarning)
                  }
                  Text(chip.label)
                    .attributeModifier(
                      orbitTextMeta(
                        this.activeFilter === chip.key
                          ? this.store.palette.accentPrimary
                          : this.store.palette.fgSecondary,
                        FontWeight.Medium
                      )
                    )
                    .maxLines(1)
                }
                .height(28)
                .layoutWeight(1)
                .justifyContent(FlexAlign.Center)
                .alignItems(VerticalAlign.Center)
                .backgroundColor(
                  this.activeFilter === chip.key
                    ? this.store.palette.accentPrimarySoft
                    : this.store.palette.subtleSurface
                )
                .borderRadius(OrbitTokens.PILL_RADIUS)
                .border({
                  width: OrbitTokens.HAIRLINE,
                  color: this.activeFilter === chip.key
                    ? this.store.palette.accentPrimary
                    : this.store.palette.hairline
                })
                .onClick(() => {
                  this.activeFilter = chip.key;
                  this.store.triggerHaptic(6);
                })
              },
              (chip: SidebarFilterChip) => chip.key
            )
          }
          .width('100%')
        }
        .width('100%')
        .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })

        List({ space: OrbitTokens.SPACE_XS }) {
          if (
            this.activeFilter === 'all' &&
            (this.normalizedQuery().length === 0 ||
              this.pinnedProjectsForHost().length > 0 ||
              this.discoveredRepos().length > 0 ||
              this.store.showDirBrowser)
          ) {
            ListItem() {
              Column({ space: OrbitTokens.SPACE_XS }) {
                Row() {
                  Text('WORKSPACES')
                    .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Bold))
                  Blank()
                  Button(this.store.showDirBrowser ? 'Close' : 'Browse')
                    .attributeModifier(
                      orbitButtonMeta(this.store.palette.accentPrimary, Color.Transparent)
                    )
                    .height(28)
                    .onClick(() => {
                      if (this.store.showDirBrowser) {
                        this.store.showDirBrowser = false;
                      } else {
                        this.store.browseDirectory(
                          this.store.activeCwd === '~' ? null : this.store.activeCwd
                        );
                      }
                    })
                }
                .width('100%')

                this.renderDirectoryBrowserPanel()

                ForEach(
                  this.pinnedProjectsForHost(),
                  (proj: ClientProjectDto) => {
                    Row({ space: OrbitTokens.SPACE_SM }) {
                      Text('★')
                        .attributeModifier(orbitTextMeta(this.store.palette.accentPrimary))
                      Text(proj.name)
                        .attributeModifier(
                          orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium)
                        )
                        .maxLines(1)
                        .textOverflow({ overflow: TextOverflow.Ellipsis })
                        .layoutWeight(1)
                      if (
                        proj.directories.length > 0 &&
                        this.store.resolveWorkspaceBranch(proj.directories[0]).length > 0
                      ) {
                        Text(this.store.resolveWorkspaceBranch(proj.directories[0]))
                          .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                          .maxLines(1)
                      }
                    }
                    .width('100%')
                    .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                    .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
                    .backgroundColor(
                      proj.directories.length > 0 &&
                        cwdPathsEquivalent(proj.directories[0], this.store.activeCwd)
                        ? this.store.palette.subtleSurface
                        : Color.Transparent
                    )
                    .borderRadius(OrbitTokens.PILL_RADIUS)
                    .alignItems(VerticalAlign.Center)
                    .onClick(() => {
                      if (proj.directories.length > 0) {
                        this.store.startNewSessionDraft(proj.directories[0]);
                      }
                    })
                    .bindContextMenu(
                      () => {
                        if (proj.directories.length > 0) {
                          this.renderWorkspaceContextMenu(proj.directories[0]);
                        }
                      },
                      ResponseType.LongPress
                    )
                  },
                  (proj: ClientProjectDto) => proj.project_id
                )

                ForEach(
                  this.discoveredRepos(),
                  (repo: DiscoveredProjectDto) => {
                    Row({ space: OrbitTokens.SPACE_SM }) {
                      Text('⎇')
                        .attributeModifier(orbitTextMono(this.store.palette.statusDirect))
                      Text(repo.name)
                        .attributeModifier(
                          orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Normal)
                        )
                        .maxLines(1)
                        .textOverflow({ overflow: TextOverflow.Ellipsis })
                        .layoutWeight(1)
                      if (this.store.resolveWorkspaceBranch(repo.path).length > 0) {
                        Text(this.store.resolveWorkspaceBranch(repo.path))
                          .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                          .maxLines(1)
                      }
                    }
                    .width('100%')
                    .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                    .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
                    .backgroundColor(
                      cwdPathsEquivalent(repo.path, this.store.activeCwd)
                        ? this.store.palette.subtleSurface
                        : Color.Transparent
                    )
                    .borderRadius(OrbitTokens.PILL_RADIUS)
                    .alignItems(VerticalAlign.Center)
                    .onClick(() => {
                      this.store.startNewSessionDraft(repo.path);
                    })
                    .bindContextMenu(
                      () => {
                        this.renderWorkspaceContextMenu(repo.path);
                      },
                      ResponseType.LongPress
                    )
                  },
                  (repo: DiscoveredProjectDto) => repo.path
                )
              }
              .width('100%')
              .padding({ bottom: OrbitTokens.SPACE_SM })
            }
          }

          if (this.activeFilter === 'all' || this.activeFilter === 'approvals') {
            if (this.approvalAgents().length > 0) {
              ListItem() {
                Text('NEEDS APPROVAL')
                  .attributeModifier(
                    orbitTextMeta(this.store.palette.statusWarning, FontWeight.Bold)
                  )
                  .padding({ top: OrbitTokens.SPACE_SM, bottom: OrbitTokens.SPACE_XS })
              }
              ForEach(
                this.approvalAgents(),
                (agent: AgentCardDto) => {
                  this.renderAgentRow(agent)
                },
                (agent: AgentCardDto) => `appr_${agent.agent_id}_${agent.latest_seq}`
              )
            } else if (this.activeFilter === 'approvals') {
              ListItem() {
                Text('No sessions awaiting approval.')
                  .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                  .padding({ top: OrbitTokens.SPACE_SM, bottom: OrbitTokens.SPACE_MD })
              }
            }
          }

          if (this.activeFilter === 'all' || this.activeFilter === 'active') {
            if (this.activeAgents().length > 0) {
              ListItem() {
                Text('ACTIVE')
                  .attributeModifier(
                    orbitTextMeta(this.store.palette.accentPrimary, FontWeight.Bold)
                  )
                  .padding({ top: OrbitTokens.SPACE_SM, bottom: OrbitTokens.SPACE_XS })
              }
              ForEach(
                this.activeAgents(),
                (agent: AgentCardDto) => {
                  this.renderAgentRow(agent)
                },
                (agent: AgentCardDto) => `act_${agent.agent_id}_${agent.latest_seq}`
              )
            } else if (this.activeFilter === 'active') {
              ListItem() {
                Text('No active sessions running.')
                  .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                  .padding({ top: OrbitTokens.SPACE_SM, bottom: OrbitTokens.SPACE_MD })
              }
            }
          }

          if (this.activeFilter === 'all') {
            ListItem() {
              Row() {
                Text('RECENT SESSIONS')
                  .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Bold))
                Blank()
                Button('New Session')
                  .attributeModifier(
                    orbitButtonMeta(this.store.palette.accentPrimary, Color.Transparent)
                  )
                  .height(28)
                  .onClick(() => {
                    this.store.startNewSessionDraft();
                  })
              }
              .width('100%')
              .padding({ top: OrbitTokens.SPACE_SM, bottom: OrbitTokens.SPACE_XS })
            }

            if (
              this.recentAgents().length === 0 &&
              this.activeAgents().length === 0 &&
              this.approvalAgents().length === 0
            ) {
              ListItem() {
                Text(
                  this.normalizedQuery().length > 0
                    ? 'No matching sessions found.'
                    : 'No sessions on this host yet.'
                )
                  .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                  .padding({ top: OrbitTokens.SPACE_XS, bottom: OrbitTokens.SPACE_MD })
              }
            } else {
              ForEach(
                this.recentAgents(),
                (agent: AgentCardDto) => {
                  this.renderAgentRow(agent)
                },
                (agent: AgentCardDto) => `rec_${agent.agent_id}_${agent.latest_seq}`
              )
            }
          }
        }
        .width('100%')
        .layoutWeight(1)
        .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
        .scrollBar(BarState.Auto)
      }

      Row({ space: OrbitTokens.SPACE_SM }) {
        Button('Pair Host')
          .attributeModifier(
            orbitButtonSubhead(this.store.palette.fgPrimary, this.store.palette.elevated)
          )
          .height(OrbitTokens.HIT_MIN)
          .borderRadius(OrbitTokens.CARD_RADIUS)
          .border({
            width: OrbitTokens.HAIRLINE,
            color: this.store.palette.hairline
          })
          .layoutWeight(1)
          .onClick(() => {
            this.store.scanQrAndPair(this.getUIContext().getHostContext());
          })

        Button('Settings')
          .attributeModifier(
            orbitButtonSubhead(this.store.palette.fgPrimary, this.store.palette.elevated)
          )
          .height(OrbitTokens.HIT_MIN)
          .borderRadius(OrbitTokens.CARD_RADIUS)
          .border({
            width: OrbitTokens.HAIRLINE,
            color: this.store.palette.hairline
          })
          .layoutWeight(1)
          .onClick(() => {
            this.store.openHostSettings();
          })
      }
      .width('100%')
      .padding({
        left: OrbitTokens.SPACE_MD,
        right: OrbitTokens.SPACE_MD,
        top: OrbitTokens.SPACE_SM,
        bottom: Math.max(OrbitTokens.SPACE_MD, this.store.safeBottomVp)
      })
      .border({
        width: { top: OrbitTokens.HAIRLINE },
        color: this.store.palette.hairline
      })
    }
    .width('100%')
    .height('100%')
    .padding({ top: Math.max(OrbitTokens.SPACE_SM, this.store.safeTopVp) })
    .backgroundColor(this.store.palette.drawer)
  }
}
