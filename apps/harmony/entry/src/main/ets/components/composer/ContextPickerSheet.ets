import { SlashCommandDto, slashCommandSymbol } from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import {
  OrbitTokens,
  orbitButtonSubhead,
  orbitInputMono,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import { OrbitSectionHeader, dismissHarmonyKeyboard } from '../common/OrbitCommonControls';

@ComponentV2
export struct ContextPickerSheet {
  @Local fileQuery: string = '';
  @Local worktreeDraft: string = '';

  private navPathStack: NavPathStack = new NavPathStack();
  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  @Computed
  get slashCommands(): SlashCommandDto[] {
    return this.store.slashCommands;
  }

  @Computed
  get filteredFiles(): string[] {
    const q = this.fileQuery.trim().toLowerCase();
    const out: string[] = [];
    for (let i = 0; i < this.store.workspaceFiles.length; i++) {
      const f = this.store.workspaceFiles[i];
      if (q.length === 0 || f.toLowerCase().indexOf(q) >= 0) {
        out.push(f);
        if (out.length >= 200) {
          break;
        }
      }
    }
    return out;
  }

  @Builder
  private renderLiveContextRow(
    badge: string,
    badgeColor: ResourceColor,
    label: string,
    onTap: () => void
  ) {
    ListItem() {
      Button({ type: ButtonType.Normal, stateEffect: true }) {
        Row({ space: OrbitTokens.SPACE_MD }) {
          Text(badge)
            .attributeModifier(orbitTextMono(badgeColor, FontWeight.Bold))
            .width(80)
          Text(label)
            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
            .layoutWeight(1)
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
          SymbolGlyph($r('sys.symbol.plus'))
            .fontSize(16)
            .fontColor([this.store.palette.accentPrimary])
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
        onTap();
      })
    }
  }

  build() {
    Navigation(this.navPathStack) {
      List() {
        // 1. Live Context
        ListItem() {
          OrbitSectionHeader({ title: 'Live Context', isFirst: true })
        }

        ListItemGroup() {
          this.renderLiveContextRow(
            '@diff',
            this.store.palette.statusDirect,
            'Working Tree Diff',
            () => {
              this.store.attachWorkingTreeDiffBadge();
              this.store.showContextPickerSheet = false;
            }
          )
          this.renderLiveContextRow(
            '@staged',
            this.store.palette.accentPrimary,
            'Staged Index Diff',
            () => {
              this.store.attachStagedDiffBadge();
              this.store.showContextPickerSheet = false;
            }
          )
          this.renderLiveContextRow(
            '@terminal',
            this.store.palette.statusRelay,
            'Active PTY Terminal Output',
            () => {
              this.store.attachTerminalOutputBadge();
              this.store.showContextPickerSheet = false;
            }
          )
        }
        .backgroundColor(this.store.palette.listCard)
        .borderRadius(OrbitTokens.RADIUS_LG)
        .clip(true)
        .divider({
          strokeWidth: OrbitTokens.HAIRLINE,
          color: this.store.palette.hairline,
          startMargin: OrbitTokens.SPACE_LG,
          endMargin: OrbitTokens.SPACE_LG
        })

        if (this.store.selectedAgentId.length === 0) {
          ListItem() {
            OrbitSectionHeader({ title: 'Worktree Branch' })
          }

          ListItem() {
            Row({ space: OrbitTokens.SPACE_SM }) {
              TextInput({
                placeholder: 'Optional branch (e.g. feat/auth)',
                text: this.worktreeDraft.length > 0 ? this.worktreeDraft : this.store.worktreeBranch
              })
                .attributeModifier(
                  orbitInputMono(
                    this.store.palette.fgPrimary,
                    this.store.palette.fgMuted,
                    this.store.palette.listCard
                  )
                )
                .height(OrbitTokens.HIT_MIN)
                .borderRadius(OrbitTokens.RADIUS_MD)
                .layoutWeight(1)
                .onChange((v: string) => {
                  this.worktreeDraft = v;
                })

              Button('Set')
                .attributeModifier(
                  orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
                )
                .height(OrbitTokens.HIT_MIN)
                .borderRadius(OrbitTokens.RADIUS_MD)
                .onClick(() => {
                  dismissHarmonyKeyboard(this.getUIContext());
                  this.store.worktreeBranch = this.worktreeDraft.trim();
                  this.store.showContextPickerSheet = false;
                })
            }
            .width('100%')
          }
        }

        // 2. Slash Commands
        ListItem() {
          OrbitSectionHeader({ title: 'Slash Commands' })
        }

        ListItemGroup() {
          ForEach(
            this.slashCommands,
            (cmd: SlashCommandDto) => {
              ListItem() {
                Button({ type: ButtonType.Normal, stateEffect: true }) {
                  Row({ space: OrbitTokens.SPACE_MD }) {
                    SymbolGlyph(slashCommandSymbol(cmd.id))
                      .fontSize(16)
                      .fontColor([this.store.palette.accentPrimary])
                    Column({ space: 2 }) {
                      Text(cmd.command)
                        .attributeModifier(orbitTextMono(this.store.palette.fgPrimary, FontWeight.Medium))
                      Text(cmd.description)
                        .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                        .maxLines(1)
                        .textOverflow({ overflow: TextOverflow.Ellipsis })
                    }
                    .alignItems(HorizontalAlign.Start)
                    .layoutWeight(1)
                  }
                  .width('100%')
                  .alignItems(VerticalAlign.Center)
                }
                .width('100%')
                .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                .padding({
                  left: OrbitTokens.SPACE_LG,
                  right: OrbitTokens.SPACE_LG,
                  top: OrbitTokens.SPACE_SM,
                  bottom: OrbitTokens.SPACE_SM
                })
                .backgroundColor(Color.Transparent)
                .onClick(() => {
                  dismissHarmonyKeyboard(this.getUIContext());
                  this.store.executeSlashCommand(cmd);
                  this.store.showContextPickerSheet = false;
                })
              }
            },
            (cmd: SlashCommandDto) => cmd.id
          )
        }
        .backgroundColor(this.store.palette.listCard)
        .borderRadius(OrbitTokens.RADIUS_LG)
        .clip(true)
        .divider({
          strokeWidth: OrbitTokens.HAIRLINE,
          color: this.store.palette.hairline,
          startMargin: OrbitTokens.SPACE_LG,
          endMargin: OrbitTokens.SPACE_LG
        })

        // 3. Workspace Files (@file)
        ListItem() {
          OrbitSectionHeader({ title: 'Workspace Files (@file)' })
        }

        ListItem() {
          Search({ value: this.fileQuery, placeholder: 'Filter files in workspace…' })
            .width('100%')
            .height(OrbitTokens.HIT_COMPACT)
            .backgroundColor(this.store.palette.listCard)
            .placeholderColor(this.store.palette.fgMuted)
            .fontColor(this.store.palette.fgPrimary)
            .textFont({ size: OrbitTokens.FONT_SUBHEAD_RES })
            .placeholderFont({ size: OrbitTokens.FONT_SUBHEAD_RES })
            .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
            .margin({ bottom: OrbitTokens.SPACE_SM })
            .onChange((val: string) => {
              this.fileQuery = val;
            })
        }

        if (this.filteredFiles.length === 0) {
          ListItem() {
            Text('No matching workspace files indexed yet.')
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
              .padding({
                left: OrbitTokens.SPACE_XS,
                top: OrbitTokens.SPACE_SM,
                bottom: OrbitTokens.SPACE_SM
              })
          }
        } else {
          ListItemGroup() {
            Repeat<string>(this.filteredFiles)
              .each((ri: RepeatItem<string>) => {
                ListItem() {
                  Button({ type: ButtonType.Normal, stateEffect: true }) {
                    Row({ space: OrbitTokens.SPACE_SM }) {
                      Text(`@${ri.item}`)
                        .attributeModifier(orbitTextMono(this.store.palette.fgPrimary))
                        .maxLines(1)
                        .textOverflow({ overflow: TextOverflow.Ellipsis })
                        .layoutWeight(1)
                      Text('Attach')
                        .attributeModifier(orbitTextMeta(this.store.palette.accentPrimary, FontWeight.Medium))
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
                    this.store.attachFileBadge(ri.item);
                    this.store.showContextPickerSheet = false;
                  })
                }
              })
              .key((filePath: string) => filePath)
              .virtualScroll()
          }
          .backgroundColor(this.store.palette.listCard)
          .borderRadius(OrbitTokens.RADIUS_LG)
          .clip(true)
          .divider({
            strokeWidth: OrbitTokens.HAIRLINE,
            color: this.store.palette.hairline,
            startMargin: OrbitTokens.SPACE_LG,
            endMargin: OrbitTokens.SPACE_LG
          })
        }
      }
      .width('100%')
      .height('100%')
      .padding({
        left: OrbitTokens.SPACE_LG,
        right: OrbitTokens.SPACE_LG,
        top: OrbitTokens.SPACE_XS,
        bottom: this.store.keyboardHeightVp > 0
          ? OrbitTokens.SPACE_MD
          : Math.max(OrbitTokens.SPACE_XL, this.store.safeBottomVp)
      })
      .scrollBar(BarState.Auto)
      .edgeEffect(EdgeEffect.Spring)
      .onScrollStart(() => {
        dismissHarmonyKeyboard(this.getUIContext());
      })
    }
    .title('Attach Context')
    .titleMode(NavigationTitleMode.Mini)
    .hideBackButton(true)
    .mode(NavigationMode.Stack)
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.drawer)
  }
}
