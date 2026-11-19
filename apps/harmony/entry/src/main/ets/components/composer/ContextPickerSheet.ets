import { SlashCommandItem } from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import {
  OrbitTokens,
  orbitButtonSubhead,
  orbitInputMono,
  orbitInputSubhead,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead,
  orbitTextTitle
} from '../../theme/OrbitTokens';

@ComponentV2
export struct ContextPickerSheet {
  @Local fileQuery: string = '';
  @Local worktreeDraft: string = '';

  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  private filteredFiles(): string[] {
    const q = this.fileQuery.trim().toLowerCase();
    const out: string[] = [];
    for (let i = 0; i < this.store.workspaceFiles.length; i++) {
      const f = this.store.workspaceFiles[i];
      if (q.length === 0 || f.toLowerCase().indexOf(q) >= 0) {
        out.push(f);
        if (out.length >= 60) {
          break;
        }
      }
    }
    return out;
  }

  build() {
    Column({ space: OrbitTokens.SPACE_LG }) {
      Row() {
        Text('Attach Context')
          .attributeModifier(orbitTextTitle(this.store.palette.fgPrimary, FontWeight.Bold))
        Blank()
        Button('Done')
          .attributeModifier(orbitButtonSubhead(this.store.palette.accentPrimary, Color.Transparent))
          .height(OrbitTokens.HIT_MIN)
          .onClick(() => {
            this.store.showContextPickerSheet = false;
          })
      }
      .width('100%')

      Scroll() {
        Column({ space: OrbitTokens.SPACE_LG }) {
          Column({ space: OrbitTokens.SPACE_SM }) {
            Text('LIVE CONTEXT')
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Bold))

            Row({ space: OrbitTokens.SPACE_MD }) {
              Text('@diff')
                .attributeModifier(orbitTextMono(this.store.palette.statusDirect, FontWeight.Bold))
                .width(76)
              Text('Working Tree Diff')
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                .layoutWeight(1)
              Text('+')
                .attributeModifier(orbitTextSubhead(this.store.palette.accentPrimary, FontWeight.Bold))
            }
            .width('100%')
            .height(OrbitTokens.HIT_MIN)
            .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
            .backgroundColor(this.store.palette.elevated)
            .borderRadius(OrbitTokens.CARD_RADIUS)
            .border({
              width: OrbitTokens.HAIRLINE,
              color: this.store.palette.hairline
            })
            .onClick(() => {
              this.store.attachWorkingTreeDiffBadge();
              this.store.showContextPickerSheet = false;
            })

            Row({ space: OrbitTokens.SPACE_MD }) {
              Text('@staged')
                .attributeModifier(orbitTextMono(this.store.palette.accentPrimary, FontWeight.Bold))
                .width(76)
              Text('Staged Index Diff')
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                .layoutWeight(1)
              Text('+')
                .attributeModifier(orbitTextSubhead(this.store.palette.accentPrimary, FontWeight.Bold))
            }
            .width('100%')
            .height(OrbitTokens.HIT_MIN)
            .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
            .backgroundColor(this.store.palette.elevated)
            .borderRadius(OrbitTokens.CARD_RADIUS)
            .border({
              width: OrbitTokens.HAIRLINE,
              color: this.store.palette.hairline
            })
            .onClick(() => {
              this.store.attachStagedDiffBadge();
              this.store.showContextPickerSheet = false;
            })

            Row({ space: OrbitTokens.SPACE_MD }) {
              Text('@terminal')
                .attributeModifier(orbitTextMono(this.store.palette.statusRelay, FontWeight.Bold))
                .width(76)
              Text('Active PTY Terminal Output')
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                .layoutWeight(1)
              Text('+')
                .attributeModifier(orbitTextSubhead(this.store.palette.accentPrimary, FontWeight.Bold))
            }
            .width('100%')
            .height(OrbitTokens.HIT_MIN)
            .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
            .backgroundColor(this.store.palette.elevated)
            .borderRadius(OrbitTokens.CARD_RADIUS)
            .border({
              width: OrbitTokens.HAIRLINE,
              color: this.store.palette.hairline
            })
            .onClick(() => {
              this.store.attachTerminalOutputBadge();
              this.store.showContextPickerSheet = false;
            })

            if (this.store.selectedAgentId.length === 0) {
              Row({ space: OrbitTokens.SPACE_SM }) {
                TextInput({
                  placeholder: 'Optional worktree branch (e.g. feat/auth)',
                  text: this.worktreeDraft.length > 0 ? this.worktreeDraft : this.store.worktreeBranch
                })
                  .attributeModifier(
                    orbitInputMono(
                      this.store.palette.fgPrimary,
                      this.store.palette.fgMuted,
                      this.store.palette.code
                    )
                  )
                  .height(OrbitTokens.HIT_MIN)
                  .borderRadius(OrbitTokens.CARD_RADIUS)
                  .border({
                    width: OrbitTokens.HAIRLINE,
                    color: this.store.palette.hairline
                  })
                  .layoutWeight(1)
                  .onChange((v: string) => {
                    this.worktreeDraft = v;
                  })

                Button('Set Branch')
                  .attributeModifier(
                    orbitButtonSubhead(this.store.palette.fgPrimary, this.store.palette.subtleSurface)
                  )
                  .height(OrbitTokens.HIT_MIN)
                  .borderRadius(OrbitTokens.CARD_RADIUS)
                  .onClick(() => {
                    this.store.worktreeBranch = this.worktreeDraft.trim();
                    this.store.showContextPickerSheet = false;
                  })
              }
              .width('100%')
            }
          }
          .alignItems(HorizontalAlign.Start)
          .width('100%')

          Column({ space: OrbitTokens.SPACE_SM }) {
            Text('SLASH COMMANDS')
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Bold))

            ForEach(
              this.store.slashCommands,
              (cmd: SlashCommandItem) => {
                Row({ space: OrbitTokens.SPACE_MD }) {
                  Text(cmd.command)
                    .attributeModifier(
                      orbitTextMono(this.store.palette.accentPrimary, FontWeight.Bold)
                    )
                    .width(76)
                  Column({ space: 2 }) {
                    Text(cmd.title)
                      .attributeModifier(
                        orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium)
                      )
                    Text(cmd.description)
                      .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                      .maxLines(1)
                      .textOverflow({ overflow: TextOverflow.Ellipsis })
                  }
                  .alignItems(HorizontalAlign.Start)
                  .layoutWeight(1)
                }
                .width('100%')
                .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                .padding(OrbitTokens.SPACE_MD)
                .backgroundColor(this.store.palette.elevated)
                .borderRadius(OrbitTokens.CARD_RADIUS)
                .border({
                  width: OrbitTokens.HAIRLINE,
                  color: this.store.palette.hairline
                })
                .onClick(() => {
                  this.store.applySlashCommand(cmd);
                })
              },
              (cmd: SlashCommandItem) => cmd.command
            )
          }
          .alignItems(HorizontalAlign.Start)
          .width('100%')

          Column({ space: OrbitTokens.SPACE_SM }) {
            Text('WORKSPACE FILES (@FILE)')
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Bold))

            TextInput({
              placeholder: 'Filter files in workspace…',
              text: this.fileQuery
            })
              .attributeModifier(
                orbitInputSubhead(
                  this.store.palette.fgPrimary,
                  this.store.palette.fgMuted,
                  this.store.palette.subtleSurface
                )
              )
              .height(OrbitTokens.HIT_MIN)
              .borderRadius(OrbitTokens.CARD_RADIUS)
              .width('100%')
              .onChange((val: string) => {
                this.fileQuery = val;
              })

            if (this.filteredFiles().length === 0) {
              Text('No matching workspace files indexed yet.')
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                .padding({ top: OrbitTokens.SPACE_SM, bottom: OrbitTokens.SPACE_SM })
            } else {
              ForEach(
                this.filteredFiles(),
                (filePath: string) => {
                  Row({ space: OrbitTokens.SPACE_SM }) {
                    Text(`@${filePath}`)
                      .attributeModifier(orbitTextMono(this.store.palette.fgPrimary))
                      .maxLines(1)
                      .textOverflow({ overflow: TextOverflow.Ellipsis })
                      .layoutWeight(1)
                    Text('Attach')
                      .attributeModifier(
                        orbitTextMeta(this.store.palette.accentPrimary, FontWeight.Medium)
                      )
                  }
                  .width('100%')
                  .height(OrbitTokens.HIT_MIN)
                  .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
                  .backgroundColor(this.store.palette.elevated)
                  .borderRadius(OrbitTokens.PILL_RADIUS)
                  .border({
                    width: OrbitTokens.HAIRLINE,
                    color: this.store.palette.hairline
                  })
                  .onClick(() => {
                    this.store.attachFileBadge(filePath);
                    this.store.showContextPickerSheet = false;
                  })
                },
                (filePath: string) => filePath
              )
            }
          }
          .alignItems(HorizontalAlign.Start)
          .width('100%')
        }
        .width('100%')
        .padding({ bottom: OrbitTokens.SPACE_XL })
      }
      .layoutWeight(1)
      .scrollBar(BarState.Auto)
    }
    .width('100%')
    .height('100%')
    .padding({
      left: OrbitTokens.SPACE_LG,
      right: OrbitTokens.SPACE_LG,
      top: OrbitTokens.SPACE_LG,
      bottom: Math.max(OrbitTokens.SPACE_LG, this.store.safeBottomVp)
    })
    .backgroundColor(this.store.palette.canvas)
  }
}
