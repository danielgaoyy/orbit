import {
  DirectoryEntryDto,
  DirectoryListingDto,
  DiscoveredProjectDto,
  HostCardDto,
  basenameOfPath,
  compactPath,
  cwdPathsEquivalent
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import {
  resolveDirectoryBrowserCurrentPath,
  resolveDirectoryBrowserParentPath
} from '../../model/OrbitDiffInspectorModel';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitButtonSubhead,
  orbitInputMono,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import {
  OrbitSectionHeader,
  dismissHarmonyKeyboard,
  resolveSelectedHost
} from '../common/OrbitCommonControls';

interface WorkspaceBreadcrumbItem {
  id: string;
  label: string;
  targetPath: string;
}

@ComponentV2
export struct DirectoryBrowserSheet {
  @Local filterQuery: string = '';
  @Local isEditingPath: boolean = false;
  @Local manualPathInput: string = '';

  private navPathStack: NavPathStack = new NavPathStack();
  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  @Computed
  get activeHostCard(): HostCardDto | null {
    return resolveSelectedHost(this.store);
  }

  @Computed
  get listing(): DirectoryListingDto | null {
    return this.store.currentHostDetail?.directory_listing ?? null;
  }

  @Computed
  get currentPath(): string {
    const l = this.listing;
    return resolveDirectoryBrowserCurrentPath(l !== null ? l.current_path : null, this.store.activeCwd);
  }

  @Computed
  get parentPath(): string | null {
    const l = this.listing;
    return resolveDirectoryBrowserParentPath(l !== null ? l.parent_path : null);
  }

  @Computed
  get normalizedFilter(): string {
    return this.filterQuery.trim().toLowerCase();
  }

  @Computed
  get filteredEntries(): DirectoryEntryDto[] {
    const l = this.listing;
    if (l === null) {
      return [];
    }
    const q = this.normalizedFilter;
    const out: DirectoryEntryDto[] = [];
    for (let i = 0; i < l.entries.length; i++) {
      const e = l.entries[i];
      if (q.length === 0 || e.name.toLowerCase().indexOf(q) >= 0) {
        out.push(e);
      }
    }
    return out;
  }

  @Computed
  get filteredRepos(): DiscoveredProjectDto[] {
    const repos = this.store.currentHostDetail?.discovered_projects ?? [];
    const q = this.normalizedFilter;
    const out: DiscoveredProjectDto[] = [];
    for (let i = 0; i < repos.length; i++) {
      const r = repos[i];
      if (
        q.length === 0 ||
        r.name.toLowerCase().indexOf(q) >= 0 ||
        r.path.toLowerCase().indexOf(q) >= 0
      ) {
        out.push(r);
      }
    }
    return out;
  }

  @Computed
  get breadcrumbs(): WorkspaceBreadcrumbItem[] {
    const raw = this.currentPath;
    if (raw === '~' || raw === '/') {
      return [{ id: 'root', label: raw, targetPath: raw }];
    }
    const isAbs = raw.startsWith('/');
    const parts = raw.split('/').filter((s: string) => s.length > 0);
    const items: WorkspaceBreadcrumbItem[] = [];
    if (isAbs) {
      items.push({ id: 'root', label: '/', targetPath: '/' });
    }
    let accum = isAbs ? '' : '';
    for (let i = 0; i < parts.length; i++) {
      const seg = parts[i];
      accum = isAbs ? `${accum}/${seg}` : (accum.length > 0 ? `${accum}/${seg}` : seg);
      items.push({
        id: `seg_${i}_${accum}`,
        label: seg,
        targetPath: accum
      });
    }
    if (items.length > 4) {
      return [
        items[0],
        items[items.length - 3],
        items[items.length - 2],
        items[items.length - 1]
      ];
    }
    return items;
  }

  @Computed
  get isCurrentPinned(): boolean {
    const path = this.currentPath;
    for (let i = 0; i < this.store.projects.length; i++) {
      const proj = this.store.projects[i];
      if (proj.host_id !== this.store.selectedHostId) {
        continue;
      }
      for (let j = 0; j < proj.directories.length; j++) {
        if (cwdPathsEquivalent(proj.directories[j], path)) {
          return true;
        }
      }
    }
    return false;
  }

  @Computed
  get currentFolderName(): string {
    const p = this.currentPath;
    if (p === '~' || p === '/') {
      return p;
    }
    const name = basenameOfPath(p);
    return name.length > 0 ? name : p;
  }

  private submitManualPath(): void {
    const target = this.manualPathInput;
    this.isEditingPath = false;
    dismissHarmonyKeyboard(this.getUIContext());
    if (target.trim().length > 0) {
      this.filterQuery = '';
      this.store.browseDirectory(target);
      this.store.triggerHapticPreset('selection');
    }
  }

  build() {
    Navigation(this.navPathStack) {
      Column({ space: 0 }) {
        // Top Breadcrumb / Manual Path + Native Search Filter
        Column({ space: OrbitTokens.SPACE_SM }) {
          if (this.isEditingPath) {
            Row({ space: OrbitTokens.SPACE_SM }) {
              SymbolGlyph($r('sys.symbol.folder'))
                .fontSize(15)
                .fontColor([this.store.palette.fgMuted])

              TextInput({
                placeholder: $r('app.string.dir_path_placeholder'),
                text: this.manualPathInput
              })
                .attributeModifier(
                  orbitInputMono(
                    this.store.palette.fgPrimary,
                    this.store.palette.fgMuted,
                    Color.Transparent
                  )
                )
                .layoutWeight(1)
                .height(OrbitTokens.HIT_COMPACT)
                .padding(0)
                .onChange((val: string) => {
                  this.manualPathInput = val;
                })
                .onSubmit(() => {
                  this.submitManualPath();
                })

              Button('Go', { type: ButtonType.Capsule, stateEffect: true })
                .attributeModifier(
                  orbitButtonMeta(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
                )
                .height(OrbitTokens.HIT_COMPACT)
                .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
                .onClick(() => {
                  this.submitManualPath();
                })

              Button({ type: ButtonType.Circle, stateEffect: true }) {
                SymbolGlyph($r('sys.symbol.xmark'))
                  .fontSize(14)
                  .renderingStrategy(SymbolRenderingStrategy.SINGLE)
                  .fontColor([this.store.palette.iconSecondary])
              }
              .width(OrbitTokens.HIT_COMPACT)
              .height(OrbitTokens.HIT_COMPACT)
              .padding(0)
              .backgroundColor(Color.Transparent)
              .accessibilityText($r('app.string.dir_cancel_path_a11y'))
              .onClick(() => {
                dismissHarmonyKeyboard(this.getUIContext());
                this.isEditingPath = false;
              })
            }
            .width('100%')
            .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
            .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_XS })
            .backgroundColor(this.store.palette.listCard)
            .borderRadius(OrbitTokens.RADIUS_MD)
            .alignItems(VerticalAlign.Center)
          } else {
            Row({ space: OrbitTokens.SPACE_XS }) {
              if (this.parentPath !== null) {
                Button({ type: ButtonType.Circle, stateEffect: true }) {
                  SymbolGlyph($r('sys.symbol.chevron_left'))
                    .fontSize(15)
                    .renderingStrategy(SymbolRenderingStrategy.SINGLE)
                    .fontColor([this.store.palette.iconPrimary])
                }
                .width(OrbitTokens.HIT_COMPACT)
                .height(OrbitTokens.HIT_COMPACT)
                .padding(0)
                .backgroundColor(this.store.palette.listCard)
                .accessibilityText($r('app.string.dir_parent_a11y'))
                .onClick(() => {
                  this.filterQuery = '';
                  this.store.browseDirectory(this.parentPath);
                  this.store.triggerHapticPreset('selection');
                })
              }

              Scroll() {
                Row({ space: 4 }) {
                  ForEach(
                    this.breadcrumbs,
                    (seg: WorkspaceBreadcrumbItem, idx: number) => {
                      Row({ space: 4 }) {
                        if (idx > 0) {
                          SymbolGlyph($r('sys.symbol.chevron_right'))
                            .fontSize(11)
                            .fontColor([this.store.palette.fgMuted])
                        }
                        Button(seg.label, { type: ButtonType.Capsule, stateEffect: true })
                          .attributeModifier(
                            orbitButtonMeta(
                              idx === this.breadcrumbs.length - 1
                                ? this.store.palette.fgPrimary
                                : this.store.palette.fgSecondary,
                              idx === this.breadcrumbs.length - 1
                                ? this.store.palette.listCard
                                : Color.Transparent
                            )
                          )
                          .height(OrbitTokens.HIT_COMPACT)
                          .padding({ left: 10, right: 10 })
                          .onClick(() => {
                            if (idx < this.breadcrumbs.length - 1) {
                              this.filterQuery = '';
                              this.store.browseDirectory(seg.targetPath);
                              this.store.triggerHapticPreset('selection');
                            }
                          })
                      }
                      .alignItems(VerticalAlign.Center)
                    },
                    (seg: WorkspaceBreadcrumbItem) => seg.id
                  )
                }
                .alignItems(VerticalAlign.Center)
              }
              .scrollable(ScrollDirection.Horizontal)
              .scrollBar(BarState.Off)
              .layoutWeight(1)

              Button({ type: ButtonType.Circle, stateEffect: true }) {
                SymbolGlyph($r('sys.symbol.pencil_line'))
                  .fontSize(15)
                  .renderingStrategy(SymbolRenderingStrategy.SINGLE)
                  .fontColor([this.store.palette.iconSecondary])
              }
              .width(OrbitTokens.HIT_COMPACT)
              .height(OrbitTokens.HIT_COMPACT)
              .padding(0)
              .backgroundColor(this.store.palette.listCard)
              .accessibilityText($r('app.string.dir_enter_path_a11y'))
              .onClick(() => {
                this.manualPathInput = this.currentPath;
                this.isEditingPath = true;
                this.store.triggerHapticPreset('selection');
              })
            }
            .width('100%')
            .alignItems(VerticalAlign.Center)
          }

          Search({ value: this.filterQuery, placeholder: $r('app.string.dir_filter_placeholder') })
            .width('100%')
            .height(OrbitTokens.HIT_COMPACT)
            .backgroundColor(this.store.palette.listCard)
            .placeholderColor(this.store.palette.fgMuted)
            .fontColor(this.store.palette.fgPrimary)
            .textFont({ size: OrbitTokens.FONT_SUBHEAD_RES })
            .placeholderFont({ size: OrbitTokens.FONT_SUBHEAD_RES })
            .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
            .onChange((val: string) => {
              this.filterQuery = val;
            })
        }
        .width('100%')
        .padding({
          left: OrbitTokens.SPACE_LG,
          right: OrbitTokens.SPACE_LG,
          top: OrbitTokens.SPACE_XS,
          bottom: OrbitTokens.SPACE_MD
        })

        // Native Grouped List with Repeat Virtualization
        List({ space: 0 }) {
          ListItem() {
            OrbitSectionHeader({ title: 'Folders', isFirst: true })
          }

          ListItemGroup() {
            if (this.listing === null) {
              ListItem() {
                Row({ space: OrbitTokens.SPACE_SM }) {
                  LoadingProgress()
                    .width(18)
                    .height(18)
                    .color(this.store.palette.iconSecondary)
                  Text($r('app.string.dir_loading'))
                    .attributeModifier(orbitTextSubhead(this.store.palette.fgSecondary))
                }
                .width('100%')
                .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
                .alignItems(VerticalAlign.Center)
              }
            } else if (this.filteredEntries.length === 0) {
              ListItem() {
                Row() {
                  Text(
                    this.normalizedFilter.length === 0
                      ? 'No subfolders in this directory'
                      : 'No matching folders'
                  )
                    .attributeModifier(orbitTextSubhead(this.store.palette.fgMuted))
                }
                .width('100%')
                .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
                .alignItems(VerticalAlign.Center)
              }
            } else {
              Repeat<DirectoryEntryDto>(this.filteredEntries)
                .each((ri: RepeatItem<DirectoryEntryDto>) => {
                  ListItem() {
                    Button({ type: ButtonType.Normal, stateEffect: true }) {
                      Row({ space: 10 }) {
                        Row() {
                          SymbolGlyph(
                            ri.item.is_git_repo
                              ? $r('sys.symbol.arrow_triangle_divide')
                              : $r('sys.symbol.folder')
                          )
                            .fontSize(16)
                            .fontColor([
                              ri.item.is_git_repo
                                ? this.store.palette.accentPrimary
                                : this.store.palette.iconSecondary
                            ])
                        }
                        .width(20)
                        .justifyContent(FlexAlign.Center)

                        Text(ri.item.name)
                          .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Normal))
                          .maxLines(1)
                          .textOverflow({ overflow: TextOverflow.Ellipsis })
                          .flexShrink(1)

                        if (ri.item.is_git_repo) {
                          Text($r('app.string.dir_git_badge'))
                            .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                            .padding({ left: 6, right: 6, top: 1, bottom: 1 })
                            .borderRadius(OrbitTokens.RADIUS_SM)
                            .backgroundColor(this.store.palette.subtleSurface)
                        }

                        Blank()

                        SymbolGlyph($r('sys.symbol.chevron_right'))
                          .fontSize(13)
                          .fontColor([this.store.palette.fgMuted])
                      }
                      .width('100%')
                      .alignItems(VerticalAlign.Center)
                    }
                    .width('100%')
                    .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                    .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
                    .backgroundColor(Color.Transparent)
                    .onClick(() => {
                      dismissHarmonyKeyboard(this.getUIContext());
                      this.filterQuery = '';
                      this.store.browseDirectory(ri.item.path);
                      this.store.triggerHapticPreset('selection');
                    })
                  }
                })
                .key((entry: DirectoryEntryDto) => entry.path)
                .virtualScroll()
            }
          }
          .backgroundColor(this.store.palette.listCard)
          .borderRadius(OrbitTokens.RADIUS_LG)
          .clip(true)
          .divider({
            strokeWidth: OrbitTokens.HAIRLINE,
            color: this.store.palette.hairline,
            startMargin: 46,
            endMargin: OrbitTokens.SPACE_LG
          })

          if (this.filteredRepos.length > 0) {
            ListItem() {
              OrbitSectionHeader({ title: 'Repositories' })
            }

            ListItemGroup() {
              Repeat<DiscoveredProjectDto>(this.filteredRepos)
                .each((ri: RepeatItem<DiscoveredProjectDto>) => {
                  ListItem() {
                    Button({ type: ButtonType.Normal, stateEffect: true }) {
                      Row({ space: 10 }) {
                        Row() {
                          SymbolGlyph($r('sys.symbol.arrow_triangle_divide'))
                            .fontSize(16)
                            .fontColor([this.store.palette.accentPrimary])
                        }
                        .width(20)
                        .justifyContent(FlexAlign.Center)

                        Column({ space: 2 }) {
                          Text(ri.item.name)
                            .attributeModifier(
                              orbitTextSubhead(
                                this.store.palette.fgPrimary,
                                cwdPathsEquivalent(ri.item.path, this.currentPath)
                                  ? FontWeight.Bold
                                  : FontWeight.Normal
                              )
                            )
                            .maxLines(1)
                            .textOverflow({ overflow: TextOverflow.Ellipsis })
                          Text(compactPath(ri.item.path))
                            .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                            .maxLines(1)
                            .textOverflow({ overflow: TextOverflow.Ellipsis })
                        }
                        .alignItems(HorizontalAlign.Start)
                        .layoutWeight(1)

                        if (ri.item.git_branch !== null && ri.item.git_branch.trim().length > 0) {
                          Text(ri.item.git_branch.trim())
                            .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                            .maxLines(1)
                            .textOverflow({ overflow: TextOverflow.Ellipsis })
                            .constraintSize({ maxWidth: 104 })
                        }
                      }
                      .width('100%')
                      .alignItems(VerticalAlign.Center)
                    }
                    .width('100%')
                    .constraintSize({ minHeight: 54 })
                    .padding({
                      left: OrbitTokens.SPACE_LG,
                      right: OrbitTokens.SPACE_LG,
                      top: 8,
                      bottom: 8
                    })
                    .backgroundColor(Color.Transparent)
                    .onClick(() => {
                      dismissHarmonyKeyboard(this.getUIContext());
                      this.store.startNewSessionDraft(ri.item.path);
                      this.store.showDirBrowser = false;
                    })
                  }
                })
                .key((repo: DiscoveredProjectDto) => repo.path)
                .virtualScroll()
            }
            .backgroundColor(this.store.palette.listCard)
            .borderRadius(OrbitTokens.RADIUS_LG)
            .clip(true)
            .divider({
              strokeWidth: OrbitTokens.HAIRLINE,
              color: this.store.palette.hairline,
              startMargin: 46,
              endMargin: OrbitTokens.SPACE_LG
            })
          }
        }
        .width('100%')
        .layoutWeight(1)
        .padding({
          left: OrbitTokens.SPACE_LG,
          right: OrbitTokens.SPACE_LG,
          top: OrbitTokens.SPACE_XS,
          bottom: OrbitTokens.SPACE_LG
        })
        .scrollBar(BarState.Auto)
        .edgeEffect(EdgeEffect.Spring)
        .onScrollStart(() => {
          dismissHarmonyKeyboard(this.getUIContext());
        })

        // Sticky Bottom Thumb-Zone Action Dock
        Divider()
          .strokeWidth(OrbitTokens.HAIRLINE)
          .color(this.store.palette.hairline)

        Column({ space: OrbitTokens.SPACE_SM }) {
          Row({ space: 6 }) {
            SymbolGlyph($r('sys.symbol.folder'))
              .fontSize(13)
              .fontColor([this.store.palette.fgMuted])
            Text(compactPath(this.currentPath))
              .attributeModifier(orbitTextMono(this.store.palette.fgSecondary))
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
              .layoutWeight(1)
          }
          .width('100%')
          .alignItems(VerticalAlign.Center)

          Row({ space: OrbitTokens.SPACE_SM }) {
            Button({ type: ButtonType.Normal, stateEffect: true }) {
              Row({ space: 6 }) {
                SymbolGlyph(
                  this.isCurrentPinned ? $r('sys.symbol.pin_fill') : $r('sys.symbol.pin')
                )
                  .fontSize(15)
                  .fontColor([
                    this.isCurrentPinned
                      ? this.store.palette.accentPrimary
                      : this.store.palette.iconPrimary
                  ])
                Text(this.isCurrentPinned ? 'Pinned' : 'Pin')
                  .attributeModifier(
                    orbitTextSubhead(
                      this.isCurrentPinned
                        ? this.store.palette.accentPrimary
                        : this.store.palette.fgPrimary,
                      FontWeight.Medium
                    )
                  )
              }
            }
            .height(OrbitTokens.HIT_MIN)
            .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
            .borderRadius(OrbitTokens.RADIUS_MD)
            .backgroundColor(this.store.palette.listCard)
            .onClick(() => {
              const path = this.currentPath;
              if (this.isCurrentPinned) {
                this.store.unpinWorkspaceByCwd(path);
              } else {
                this.store.pinDirectoryAsProject(path);
              }
            })

            Button(`Open ${this.currentFolderName}`)
              .attributeModifier(
                orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
              )
              .layoutWeight(1)
              .height(OrbitTokens.HIT_MIN)
              .borderRadius(OrbitTokens.RADIUS_MD)
              .onClick(() => {
                dismissHarmonyKeyboard(this.getUIContext());
                this.store.startNewSessionDraft(this.currentPath);
                this.store.showDirBrowser = false;
              })
          }
          .width('100%')
        }
        .width('100%')
        .padding({
          left: OrbitTokens.SPACE_LG,
          right: OrbitTokens.SPACE_LG,
          top: OrbitTokens.SPACE_MD,
          bottom: this.store.keyboardHeightVp > 0
            ? OrbitTokens.SPACE_SM
            : Math.max(OrbitTokens.SPACE_MD, this.store.safeBottomVp)
        })
        .backgroundColor(this.store.palette.drawer)
      }
      .width('100%')
      .height('100%')
    }
    .title(
      this.store.hosts.length > 1 && this.activeHostCard !== null
        ? { main: 'Choose Workspace', sub: this.activeHostCard.display_name }
        : 'Choose Workspace'
    )
    .titleMode(NavigationTitleMode.Mini)
    .hideBackButton(true)
    .mode(NavigationMode.Stack)
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.drawer)
  }
}
