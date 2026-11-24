import {
  HighlightedLineDto,
  HighlightedTokenDto,
  WorkspaceFileViewDto,
  basenameOfPath,
  stripWorkspacePrefix
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitButtonSubhead,
  orbitInputMono,
  orbitSpanMonoFont,
  orbitTextBody,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead,
  resolveSemanticFileSymbol
} from '../../theme/OrbitTokens';
import { OrbitTheme } from '../../theme/OrbitTheme';
import { OrbitSheetHeader, dismissHarmonyKeyboard } from '../common/OrbitCommonControls';
import { RichMarkdownView } from '../stage/RichMarkdownView';

@ComponentV2
export struct FileExplorerSheet {
  @Local private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();
  @Local private searchQuery: string = '';
  @Local private showCreateForm: boolean = false;
  @Local private createPathInput: string = '';
  @Local private createAsDir: boolean = false;
  @Local private isEditingFile: boolean = false;
  @Local private editBuffer: string = '';
  @Local private showRenameForm: boolean = false;
  @Local private renameInput: string = '';
  @Local private confirmDeletePath: string = '';

  @Computed
  get filteredFiles(): string[] {
    const q = this.searchQuery.trim().toLowerCase();
    const all = this.store.workspaceFiles;
    if (q.length === 0) {
      return all.slice(0, 200);
    }
    const out: string[] = [];
    for (let i = 0; i < all.length && out.length < 200; i++) {
      const f = all[i];
      if (f.toLowerCase().indexOf(q) >= 0) {
        out.push(f);
      }
    }
    return out;
  }

  private formatBytes(bytes: number): string {
    if (bytes < 1024) {
      return `${bytes} B`;
    }
    if (bytes < 1024 * 1024) {
      return `${(bytes / 1024).toFixed(1)} KB`;
    }
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  private startEdit(view: WorkspaceFileViewDto): void {
    this.editBuffer = view.content;
    this.isEditingFile = true;
    this.showRenameForm = false;
    this.confirmDeletePath = '';
  }

  private async handleSaveFile(view: WorkspaceFileViewDto): Promise<void> {
    dismissHarmonyKeyboard(this.getUIContext());
    const ok = await this.store.saveWorkspaceFile(view.path, this.editBuffer);
    if (ok) {
      this.isEditingFile = false;
    }
  }

  private async handleCreateEntry(): Promise<void> {
    const clean = this.createPathInput.trim();
    if (clean.length === 0) {
      return;
    }
    dismissHarmonyKeyboard(this.getUIContext());
    const ok = await this.store.createWorkspaceEntry(clean, this.createAsDir, this.createAsDir ? null : '');
    if (ok) {
      this.createPathInput = '';
      this.createAsDir = false;
      this.showCreateForm = false;
      if (!this.createAsDir) {
        this.store.openWorkspaceFileInViewer(clean);
      }
    }
  }

  private async handleRename(oldPath: string): Promise<void> {
    const clean = this.renameInput.trim();
    if (clean.length === 0 || clean === oldPath) {
      this.showRenameForm = false;
      return;
    }
    dismissHarmonyKeyboard(this.getUIContext());
    const ok = await this.store.renameWorkspaceEntry(oldPath, clean);
    if (ok) {
      this.showRenameForm = false;
      this.renameInput = '';
    }
  }

  private async handleDelete(targetPath: string): Promise<void> {
    const ok = await this.store.deleteWorkspaceEntry(targetPath);
    if (ok) {
      this.confirmDeletePath = '';
      this.isEditingFile = false;
    }
  }

  @Builder
  private renderFileViewer(view: WorkspaceFileViewDto) {
    Column({ space: OrbitTokens.SPACE_SM }) {
      // Viewer top action bar
      Row({ space: OrbitTokens.SPACE_XS }) {
        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.chevron_left'))
            .fontSize(16)
            .fontColor([this.store.palette.iconPrimary])
        }
        .width(36)
        .height(36)
        .padding(0)
        .backgroundColor(this.store.palette.subtleSurface)
        .onClick(() => {
          this.isEditingFile = false;
          this.showRenameForm = false;
          this.confirmDeletePath = '';
          this.store.inspectedFilePath = '';
          this.store.inspectedFileView = null;
        })

        Column({ space: 2 }) {
          Text(basenameOfPath(view.path))
            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
          Text(`${view.language} · ${this.formatBytes(view.size_bytes)}`)
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
            .maxLines(1)
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        if (this.isEditingFile) {
          Button($r('app.string.action_cancel'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface))
            .height(34)
            .padding({ left: 10, right: 10 })
            .onClick(() => {
              this.isEditingFile = false;
            })
          Button($r('app.string.files_save_file'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(orbitButtonMeta(this.store.palette.onAccent, this.store.palette.accentPrimaryFill))
            .height(34)
            .padding({ left: 12, right: 12 })
            .enabled(!this.store.fileExplorerBusy)
            .onClick(() => {
              this.handleSaveFile(view);
            })
        } else {
          Button($r('app.string.files_edit_mode'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface))
            .height(34)
            .padding({ left: 10, right: 10 })
            .onClick(() => {
              this.startEdit(view);
            })
          Button($r('app.string.action_quote'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.accentPrimarySoft))
            .height(34)
            .padding({ left: 10, right: 10 })
            .onClick(() => {
              this.store.attachFileBadge(stripWorkspacePrefix(view.path, this.store.activeCwd));
              this.store.showFileExplorerSheet = false;
              this.store.showToast(`Attached @${basenameOfPath(view.path)}`, false);
            })
        }
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)

      // Secondary file management strip: Copy, Rename, Delete
      if (!this.isEditingFile) {
        Row({ space: OrbitTokens.SPACE_XS }) {
          Button($r('app.string.action_copy'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface))
            .height(30)
            .padding({ left: 10, right: 10 })
            .onClick(() => {
              this.store.copyToClipboard(view.content, `Copied ${basenameOfPath(view.path)}`);
            })

          Button($r('app.string.action_rename'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface))
            .height(30)
            .padding({ left: 10, right: 10 })
            .onClick(() => {
              this.renameInput = view.path;
              this.showRenameForm = !this.showRenameForm;
              this.confirmDeletePath = '';
            })

          Button($r('app.string.action_delete'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(orbitButtonMeta(this.store.palette.statusDanger, this.store.palette.statusDangerSoft))
            .height(30)
            .padding({ left: 10, right: 10 })
            .onClick(() => {
              this.confirmDeletePath = view.path;
              this.showRenameForm = false;
            })
        }
        .width('100%')
      }

      if (this.showRenameForm) {
        Row({ space: OrbitTokens.SPACE_SM }) {
          TextInput({ text: this.renameInput, placeholder: $r('app.string.files_new_path_placeholder') })
            .attributeModifier(
              orbitInputMono(this.store.palette.fgPrimary, this.store.palette.fgMuted, this.store.palette.recessed)
            )
            .height(OrbitTokens.HIT_COMPACT)
            .borderRadius(OrbitTokens.RADIUS_MD)
            .layoutWeight(1)
            .onChange((val: string) => {
              this.renameInput = val;
            })
          Button($r('app.string.action_confirm'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(orbitButtonMeta(this.store.palette.onAccent, this.store.palette.accentPrimaryFill))
            .height(OrbitTokens.HIT_COMPACT)
            .padding({ left: 12, right: 12 })
            .onClick(() => {
              this.handleRename(view.path);
            })
        }
        .width('100%')
      }

      if (this.confirmDeletePath.length > 0) {
        Row({ space: OrbitTokens.SPACE_SM }) {
          Text(`Delete ${basenameOfPath(this.confirmDeletePath)}?`)
            .attributeModifier(orbitTextMeta(this.store.palette.statusDanger, FontWeight.Medium))
            .layoutWeight(1)
          Button($r('app.string.action_cancel'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface))
            .height(30)
            .padding({ left: 10, right: 10 })
            .onClick(() => {
              this.confirmDeletePath = '';
            })
          Button($r('app.string.action_delete'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(orbitButtonMeta(this.store.palette.onAccent, this.store.palette.statusDanger))
            .height(30)
            .padding({ left: 10, right: 10 })
            .onClick(() => {
              this.handleDelete(this.confirmDeletePath);
            })
        }
        .width('100%')
        .padding(OrbitTokens.SPACE_SM)
        .backgroundColor(this.store.palette.statusDangerSoft)
        .borderRadius(OrbitTokens.RADIUS_MD)
      }

      if (view.truncated) {
        Text($r('app.string.files_truncated_notice'))
          .attributeModifier(orbitTextMeta(this.store.palette.statusWarning))
          .width('100%')
      }

      if (this.isEditingFile) {
        TextArea({ text: this.editBuffer })
          .fontSize(OrbitTokens.FONT_SUBHEAD)
          .fontFamily(OrbitTokens.FAMILY_MONO)
          .fontColor(this.store.palette.fgPrimary)
          .backgroundColor(this.store.palette.code)
          .borderRadius(OrbitTokens.RADIUS_MD)
          .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
          .padding(OrbitTokens.SPACE_SM)
          .width('100%')
          .layoutWeight(1)
          .onChange((val: string) => {
            this.editBuffer = val;
          })
      } else if (view.markdown_blocks.length > 0 && view.language.toLowerCase() === 'markdown') {
        Scroll() {
          Column() {
            RichMarkdownView({
              blocks: view.markdown_blocks,
              fallbackText: view.content
            })
          }
          .width('100%')
          .alignItems(HorizontalAlign.Start)
          .padding(OrbitTokens.SPACE_MD)
        }
        .width('100%')
        .layoutWeight(1)
        .backgroundColor(this.store.palette.listCard)
        .borderRadius(OrbitTokens.RADIUS_MD)
        .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
        .scrollBar(BarState.Auto)
      } else if (view.highlighted_lines.length > 0) {
        List({ space: 2 }) {
          Repeat<HighlightedLineDto>(view.highlighted_lines)
            .each((ri: RepeatItem<HighlightedLineDto>) => {
              ListItem() {
                Row({ space: OrbitTokens.SPACE_SM }) {
                  Text(ri.item.line_number.toString().padStart(4, ' '))
                    .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                    .width(36)
                    .textAlign(TextAlign.End)
                  Text() {
                    ForEach(
                      ri.item.tokens,
                      (tok: HighlightedTokenDto) => {
                        Span(tok.text)
                          .font(orbitSpanMonoFont(tok.bold ? FontWeight.Bold : FontWeight.Normal))
                          .fontColor(tok.fg_hex.length > 0 ? tok.fg_hex : this.store.palette.fgPrimary)
                      },
                      (tok: HighlightedTokenDto, tIdx: number) => `${ri.item.line_number}_${tIdx}_${tok.text.length}`
                    )
                  }
                  .copyOption(CopyOptions.LocalDevice)
                  .layoutWeight(1)
                }
                .width('100%')
                .alignItems(VerticalAlign.Top)
              }
            })
            .key((ln: HighlightedLineDto) => `hl_${ln.line_number}`)
            .virtualScroll()
        }
        .width('100%')
        .layoutWeight(1)
        .padding(OrbitTokens.SPACE_SM)
        .backgroundColor(this.store.palette.code)
        .borderRadius(OrbitTokens.RADIUS_MD)
        .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
        .scrollBar(BarState.Auto)
      } else {
        Scroll() {
          Text(view.content)
            .attributeModifier(orbitTextMono(this.store.palette.fgPrimary))
            .copyOption(CopyOptions.LocalDevice)
            .width('100%')
            .padding(OrbitTokens.SPACE_MD)
        }
        .width('100%')
        .layoutWeight(1)
        .backgroundColor(this.store.palette.code)
        .borderRadius(OrbitTokens.RADIUS_MD)
        .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
        .scrollBar(BarState.Auto)
      }
    }
    .width('100%')
    .layoutWeight(1)
    .padding({
      left: OrbitTokens.SPACE_LG,
      right: OrbitTokens.SPACE_LG,
      bottom: Math.max(OrbitTokens.SPACE_MD, this.store.safeBottomVp)
    })
  }

  @Builder
  private renderFileList() {
    Column({ space: OrbitTokens.SPACE_SM }) {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Search({ value: this.searchQuery, placeholder: $r('app.string.files_search_placeholder') })
          .layoutWeight(1)
          .height(OrbitTokens.HIT_COMPACT)
          .backgroundColor(this.store.palette.listCard)
          .placeholderColor(this.store.palette.fgMuted)
          .fontColor(this.store.palette.fgPrimary)
          .onChange((val: string) => {
            this.searchQuery = val;
          })

        Button($r('app.string.files_new_entry'), { type: ButtonType.Capsule, stateEffect: true })
          .attributeModifier(orbitButtonMeta(this.store.palette.onAccent, this.store.palette.accentPrimaryFill))
          .height(OrbitTokens.HIT_COMPACT)
          .padding({ left: 12, right: 12 })
          .onClick(() => {
            this.showCreateForm = !this.showCreateForm;
          })
      }
      .width('100%')

      if (this.showCreateForm) {
        Column({ space: OrbitTokens.SPACE_SM }) {
          TextInput({
            text: this.createPathInput,
            placeholder: $r('app.string.files_new_path_placeholder')
          })
            .attributeModifier(
              orbitInputMono(this.store.palette.fgPrimary, this.store.palette.fgMuted, this.store.palette.recessed)
            )
            .height(OrbitTokens.HIT_COMPACT)
            .borderRadius(OrbitTokens.RADIUS_MD)
            .width('100%')
            .onChange((val: string) => {
              this.createPathInput = val;
            })

          Row({ space: OrbitTokens.SPACE_SM }) {
            Toggle({ type: ToggleType.Switch, isOn: this.createAsDir })
              .selectedColor(this.store.palette.accentPrimaryFill)
              .onChange((checked: boolean) => {
                this.createAsDir = checked;
              })
            Text($r('app.string.files_create_as_dir'))
              .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
              .layoutWeight(1)
            Button($r('app.string.action_create'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(orbitButtonMeta(this.store.palette.onAccent, this.store.palette.accentPrimaryFill))
              .height(34)
              .padding({ left: 14, right: 14 })
              .enabled(!this.store.fileExplorerBusy && this.createPathInput.trim().length > 0)
              .onClick(() => {
                this.handleCreateEntry();
              })
          }
          .width('100%')
          .alignItems(VerticalAlign.Center)
        }
        .width('100%')
        .padding(OrbitTokens.SPACE_MD)
        .borderRadius(OrbitTokens.RADIUS_MD)
        .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
      }

      if (this.store.fileExplorerBusy && this.filteredFiles.length === 0) {
        Column({ space: OrbitTokens.SPACE_SM }) {
          Blank()
          LoadingProgress()
            .width(24)
            .height(24)
            .color(this.store.palette.accentPrimary)
          Blank()
        }
        .width('100%')
        .layoutWeight(1)
        .alignItems(HorizontalAlign.Center)
      } else if (this.filteredFiles.length === 0) {
        Column({ space: OrbitTokens.SPACE_SM }) {
          Blank()
          SymbolGlyph($r('sys.symbol.folder'))
            .fontSize(26)
            .fontColor([this.store.palette.fgMuted])
          Text($r('app.string.files_empty'))
            .attributeModifier(orbitTextBody(this.store.palette.fgSecondary))
          Blank()
        }
        .width('100%')
        .layoutWeight(1)
        .alignItems(HorizontalAlign.Center)
      } else {
        List({ space: OrbitTokens.SPACE_XS }) {
          Repeat<string>(this.filteredFiles)
            .each((ri: RepeatItem<string>) => {
              ListItem() {
                Button({ type: ButtonType.Normal, stateEffect: true }) {
                  Row({ space: OrbitTokens.SPACE_SM }) {
                    SymbolGlyph(resolveSemanticFileSymbol(basenameOfPath(ri.item)))
                      .fontSize(15)
                      .fontColor([this.store.palette.iconSecondary])
                    Column({ space: 2 }) {
                      Text(basenameOfPath(ri.item))
                        .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                        .maxLines(1)
                        .textOverflow({ overflow: TextOverflow.Ellipsis })
                      if (ri.item.indexOf('/') >= 0) {
                        Text(ri.item)
                          .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                          .maxLines(1)
                          .textOverflow({ overflow: TextOverflow.Ellipsis })
                      }
                    }
                    .alignItems(HorizontalAlign.Start)
                    .layoutWeight(1)
                    SymbolGlyph($r('sys.symbol.chevron_right'))
                      .fontSize(12)
                      .fontColor([this.store.palette.fgMuted])
                  }
                  .width('100%')
                  .alignItems(VerticalAlign.Center)
                }
                .width('100%')
                .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD, top: 6, bottom: 6 })
                .borderRadius(OrbitTokens.RADIUS_MD)
                .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
                .onClick(() => {
                  dismissHarmonyKeyboard(this.getUIContext());
                  this.store.openWorkspaceFileInViewer(ri.item);
                })
              }
            })
            .key((filePath: string) => filePath)
            .virtualScroll()
        }
        .width('100%')
        .layoutWeight(1)
        .scrollBar(BarState.Auto)
        .edgeEffect(EdgeEffect.Spring)
      }
    }
    .width('100%')
    .layoutWeight(1)
    .padding({
      left: OrbitTokens.SPACE_LG,
      right: OrbitTokens.SPACE_LG,
      bottom: Math.max(OrbitTokens.SPACE_MD, this.store.safeBottomVp)
    })
  }

  build() {
    Column({ space: 0 }) {
      OrbitSheetHeader({
        title: $r('app.string.files_title'),
        subtitle: basenameOfPath(this.store.activeCwd),
        onClose: () => {
          this.store.showFileExplorerSheet = false;
        }
      })

      if (this.store.inspectedFileView !== null) {
        this.renderFileViewer(this.store.inspectedFileView)
      } else {
        this.renderFileList()
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.canvas)
  }
}
