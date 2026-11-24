import { SymbolGlyphModifier } from '@kit.ArkUI';
import { common } from '@kit.AbilityKit';
import { camera, cameraPicker } from '@kit.CameraKit';
import { picker } from '@kit.CoreFileKit';
import { KeyCode } from '@kit.InputKit';
import { photoAccessHelper } from '@kit.MediaLibraryKit';
import {
  ComposerAttachmentItem,
  MentionCandidateDto,
  PromptBadgeDto,
  QueuedPromptItem,
  QuotedDiffSnippet,
  SessionPlanStepDto,
  SlashCommandDto,
  formatModeLabel,
  normalizeModeSlug,
  slashCommandSymbol
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { orbitLogWarn } from '../../model/OrbitLogger';
import { OrbitTheme } from '../../theme/OrbitTheme';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitComposerArea,
  orbitSurface,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead,
  orbitTintColorFilter,
  resolveSemanticFileSymbol
} from '../../theme/OrbitTokens';
import { dismissHarmonyKeyboard } from '../common/OrbitCommonControls';
import { ModelPickerSheet, OrbitProviderBrandIcon } from './ModelPickerSheet';

interface AutocompleteTriggerState {
  kind: string; // 'slash' | 'mention'
  startIndex: number;
  endIndex: number;
  query: string;
  rawToken: string;
}

@ComponentV2
export struct OmniComposer {
  @Local dismissedTriggerToken: string = '';
  @Local planShelfExpanded: boolean = false;

  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  @Computed
  get activeTrigger(): AutocompleteTriggerState | null {
    return this.detectTrigger();
  }

  @Computed
  get slashCandidates(): SlashCommandDto[] {
    const trig = this.activeTrigger;
    if (trig === null || trig.kind !== 'slash') {
      return [];
    }
    return this.store.filterSlashCommands(trig.query);
  }

  @Computed
  get mentionCandidates(): MentionCandidateDto[] {
    const trig = this.activeTrigger;
    if (trig === null || trig.kind !== 'mention') {
      return [];
    }
    return this.store.rankMentionCandidates(trig.query, 40);
  }

  private permissionModeIcon(mode: string): Resource {
    const slug = normalizeModeSlug(mode);
    if (slug === 'auto' || slug === 'accept_edits') {
      return $r('app.media.ic_shield_check');
    }
    if (slug === 'full_access') {
      return $r('app.media.ic_shield_off');
    }
    return $r('app.media.ic_shield');
  }

  @Builder
  private buildModelPickerSheet() {
    ModelPickerSheet()
  }

  private isAgentRunning(): boolean {
    return this.store.isActiveAgentRunning();
  }

  private canSend(): boolean {
    return this.store.hasComposerDraftContent();
  }

  private isWhitespaceChar(ch: string): boolean {
    return ch === ' ' || ch === '\n' || ch === '\t' || ch === '\r';
  }

  private detectTrigger(): AutocompleteTriggerState | null {
    const text = this.store.composerText;
    if (text.length === 0) {
      return null;
    }

    let atTrigger: AutocompleteTriggerState | null = null;
    const lastAt = text.lastIndexOf('@');
    if (lastAt >= 0 && (lastAt === 0 || this.isWhitespaceChar(text.charAt(lastAt - 1)))) {
      const q = text.substring(lastAt + 1);
      let hasSpace = false;
      for (let i = 0; i < q.length; i++) {
        if (this.isWhitespaceChar(q.charAt(i))) {
          hasSpace = true;
          break;
        }
      }
      if (!hasSpace) {
        const tok = `@${q}`;
        if (this.dismissedTriggerToken !== tok) {
          atTrigger = {
            kind: 'mention',
            startIndex: lastAt,
            endIndex: text.length,
            query: q.toLowerCase(),
            rawToken: tok
          };
        }
      }
    }

    let slashTrigger: AutocompleteTriggerState | null = null;
    if (this.store.activeSlashDirective.length === 0) {
      const lastSlash = text.lastIndexOf('/');
      if (lastSlash >= 0 && (lastSlash === 0 || this.isWhitespaceChar(text.charAt(lastSlash - 1)))) {
        const q = text.substring(lastSlash + 1);
        let hasInvalid = false;
        for (let i = 0; i < q.length; i++) {
          const ch = q.charAt(i);
          if (this.isWhitespaceChar(ch) || ch === '/') {
            hasInvalid = true;
            break;
          }
        }
        if (!hasInvalid) {
          const tok = `/${q}`;
          if (this.dismissedTriggerToken !== tok) {
            slashTrigger = {
              kind: 'slash',
              startIndex: lastSlash,
              endIndex: text.length,
              query: q.toLowerCase(),
              rawToken: tok
            };
          }
        }
      }
    }

    if (atTrigger !== null && slashTrigger !== null) {
      return atTrigger.startIndex >= slashTrigger.startIndex ? atTrigger : slashTrigger;
    }
    return atTrigger !== null ? atTrigger : slashTrigger;
  }

  private async pickPhotos(): Promise<void> {
    try {
      const opts = new photoAccessHelper.PhotoSelectOptions();
      opts.MIMEType = photoAccessHelper.PhotoViewMIMETypes.IMAGE_TYPE;
      opts.maxSelectNumber = 4;
      const photoPicker = new photoAccessHelper.PhotoViewPicker();
      const result = await photoPicker.select(opts);
      const uris: string[] = result.photoUris;
      for (let i = 0; i < uris.length; i++) {
        await this.store.attachPhotoUri(uris[i]);
      }
    } catch (e) {
      orbitLogWarn('OrbitComposer', 'PhotoViewPicker error', e as Object);
    }
  }

  private async launchCamera(): Promise<void> {
    try {
      const hostCtx = this.getUIContext().getHostContext() as common.Context;
      const profile: cameraPicker.PickerProfile = {
        cameraPosition: camera.CameraPosition.CAMERA_POSITION_BACK
      };
      const result = await cameraPicker.pick(hostCtx, [cameraPicker.PickerMediaType.PHOTO], profile);
      if (result.resultCode === 0 && result.resultUri.length > 0) {
        await this.store.attachPhotoUri(result.resultUri);
      }
    } catch (e) {
      orbitLogWarn('OrbitComposer', 'cameraPicker error', e as Object);
    }
  }

  private async pickFiles(): Promise<void> {
    try {
      const hostCtx = this.getUIContext().getHostContext() as common.Context;
      const docOpts = new picker.DocumentSelectOptions();
      docOpts.maxSelectNumber = 4;
      const docPicker = new picker.DocumentViewPicker(hostCtx);
      const uris: string[] = await docPicker.select(docOpts);
      for (let i = 0; i < uris.length; i++) {
        await this.store.attachDocumentUri(uris[i]);
      }
    } catch (e) {
      orbitLogWarn('OrbitComposer', 'DocumentViewPicker error', e as Object);
    }
  }

  private candidateSymbol(candidate: MentionCandidateDto): Resource {
    if (candidate.kind === 'git_diff' || candidate.kind === 'staged') {
      return $r('sys.symbol.arrow_left_arrow_right');
    }
    if (candidate.kind === 'terminal') {
      return $r('sys.symbol.code');
    }
    return resolveSemanticFileSymbol(candidate.title);
  }

  private selectSlashCommand(cmd: SlashCommandDto, trig: AutocompleteTriggerState): void {
    const text = this.store.composerText;
    const before = text.substring(0, trig.startIndex);
    const after = text.substring(trig.endIndex).trimStart();
    const cleaned = `${before}${after}`.trimStart();
    this.store.composerText = cleaned;
    this.dismissedTriggerToken = '';
    this.store.executeSlashCommand(cmd);
  }

  private selectMentionCandidate(
    candidate: MentionCandidateDto,
    trig: AutocompleteTriggerState
  ): void {
    const text = this.store.composerText;
    const before = text.substring(0, trig.startIndex);
    const after = text.substring(trig.endIndex);
    const token = candidate.token_label;
    this.store.composerText = `${before}${token} ${after}`;
    this.dismissedTriggerToken = '';
    if (candidate.kind === 'git_diff') {
      this.store.registerInlineMention(token, 'git_diff', '', false, 'Working Tree Diff');
    } else if (candidate.kind === 'staged') {
      this.store.registerInlineMention(token, 'git_diff', '', true, 'Staged Diff');
    } else if (candidate.kind === 'terminal') {
      this.store.registerInlineMention(token, 'terminal', '', false, 'Terminal');
    } else if (candidate.kind === 'file') {
      this.store.registerInlineMention(token, 'file', candidate.file_path, false, candidate.title);
    }
  }

  private badgeSymbol(kind: string, label: string): Resource {
    if (kind === 'image') {
      return $r('sys.symbol.picture');
    }
    if (kind === 'diff' || kind === 'staged' || kind === 'git_diff') {
      return $r('sys.symbol.arrow_left_arrow_right');
    }
    if (kind === 'terminal') {
      return $r('sys.symbol.code');
    }
    if (kind === 'quote') {
      return $r('sys.symbol.quote');
    }
    return resolveSemanticFileSymbol(label.startsWith('@') ? label.substring(1) : label);
  }

  @Builder
  private renderPlusAttachmentMenu() {
    Menu() {
      MenuItem({
        symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.picture'))
          .fontColor([this.store.palette.iconPrimary]),
        content: 'Photos'
      })
        .onClick(() => {
          this.pickPhotos();
        })
      MenuItem({
        symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.camera'))
          .fontColor([this.store.palette.iconPrimary]),
        content: 'Camera'
      })
        .onClick(() => {
          this.launchCamera();
        })
      MenuItem({
        symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.paperclip'))
          .fontColor([this.store.palette.iconPrimary]),
        content: 'Files'
      })
        .onClick(() => {
          this.pickFiles();
        })
    }
  }

  @Builder
  private renderPermissionModeMenu() {
    Menu() {
      MenuItemGroup({ header: 'Access Permissions' }) {
        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.security_shield'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Always Ask',
          labelInfo: 'Ask before edits',
          symbolEndIcon: normalizeModeSlug(this.store.composerMode) === 'default'
            ? new SymbolGlyphModifier($r('sys.symbol.checkmark')).fontColor([this.store.palette.accentPrimary])
            : undefined
        })
          .labelFontColor(this.store.palette.fgMuted)
          .onClick(() => {
            this.store.setSessionMode('default');
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.checkmark_shield'))
            .fontColor([this.store.palette.accentPrimary]),
          content: 'Auto',
          labelInfo: 'Guardian policy',
          symbolEndIcon: normalizeModeSlug(this.store.composerMode) === 'auto'
            ? new SymbolGlyphModifier($r('sys.symbol.checkmark')).fontColor([this.store.palette.accentPrimary])
            : undefined
        })
          .labelFontColor(this.store.palette.fgMuted)
          .onClick(() => {
            this.store.setSessionMode('auto');
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.checkmark_shield'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Accept Edits',
          labelInfo: 'Auto-approve edits',
          symbolEndIcon: normalizeModeSlug(this.store.composerMode) === 'accept_edits'
            ? new SymbolGlyphModifier($r('sys.symbol.checkmark')).fontColor([this.store.palette.accentPrimary])
            : undefined
        })
          .labelFontColor(this.store.palette.fgMuted)
          .onClick(() => {
            this.store.setSessionMode('accept_edits');
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.shield_checkered'))
            .fontColor([this.store.palette.statusWarning]),
          content: 'Full Access',
          labelInfo: 'No prompts',
          symbolEndIcon: normalizeModeSlug(this.store.composerMode) === 'full_access'
            ? new SymbolGlyphModifier($r('sys.symbol.checkmark')).fontColor([this.store.palette.accentPrimary])
            : undefined
        })
          .labelFontColor(this.store.palette.fgMuted)
          .onClick(() => {
            this.store.setSessionMode('full_access');
          })
      }
    }
  }

  @Builder
  private renderAutocompletePopover() {
    if (this.activeTrigger !== null) {
      if (this.activeTrigger.kind === 'slash' && this.slashCandidates.length > 0) {
        Column() {
          List({ space: 0 }) {
            ForEach(
              this.slashCandidates,
              (cmd: SlashCommandDto) => {
                ListItem() {
                  Button({ type: ButtonType.Normal, stateEffect: true }) {
                    Row({ space: OrbitTokens.SPACE_SM }) {
                      SymbolGlyph(slashCommandSymbol(cmd.id))
                        .fontSize(16)
                        .fontColor([this.store.palette.iconSecondary])
                      Text(cmd.command)
                        .attributeModifier(orbitTextMono(this.store.palette.fgPrimary, FontWeight.Medium))
                      Text(cmd.description)
                        .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                        .maxLines(1)
                        .textOverflow({ overflow: TextOverflow.Ellipsis })
                        .layoutWeight(1)
                    }
                    .width('100%')
                    .alignItems(VerticalAlign.Center)
                  }
                  .width('100%')
                  .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                  .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
                  .backgroundColor(Color.Transparent)
                  .onClick(() => {
                    const curTrig = this.activeTrigger;
                    if (curTrig !== null) {
                      this.selectSlashCommand(cmd, curTrig);
                    }
                  })
                }
              },
              (cmd: SlashCommandDto) => `slash_${cmd.id}`
            )
          }
          .width('100%')
          .constraintSize({ maxHeight: 232 })
          .scrollBar(BarState.Off)
        }
        .width('100%')
        .borderRadius(OrbitTokens.RADIUS_LG)
        .clip(true)
        .attributeModifier(
          orbitSurface(
            this.store.palette.listCard,
            this.store.palette.hairline,
            OrbitTokens.HAIRLINE,
            ShadowStyle.OUTER_DEFAULT_MD
          )
        )
      } else if (this.activeTrigger.kind === 'mention' && this.mentionCandidates.length > 0) {
        Column() {
          List({ space: 0 }) {
            ForEach(
              this.mentionCandidates,
              (cand: MentionCandidateDto) => {
                ListItem() {
                  Button({ type: ButtonType.Normal, stateEffect: true }) {
                    Row({ space: OrbitTokens.SPACE_SM }) {
                      SymbolGlyph(this.candidateSymbol(cand))
                        .fontSize(16)
                        .fontColor([this.store.palette.iconSecondary])
                      Text(cand.title)
                        .attributeModifier(orbitTextMono(this.store.palette.fgPrimary, FontWeight.Medium))
                        .maxLines(1)
                      Text(cand.subtitle)
                        .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                        .maxLines(1)
                        .textOverflow({ overflow: TextOverflow.Ellipsis })
                        .layoutWeight(1)
                    }
                    .width('100%')
                    .alignItems(VerticalAlign.Center)
                  }
                  .width('100%')
                  .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                  .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
                  .backgroundColor(Color.Transparent)
                  .onClick(() => {
                    const curTrig = this.activeTrigger;
                    if (curTrig !== null) {
                      this.selectMentionCandidate(cand, curTrig);
                    }
                  })
                }
              },
              (cand: MentionCandidateDto) => cand.key
            )
          }
          .width('100%')
          .constraintSize({ maxHeight: 232 })
          .scrollBar(BarState.Off)
        }
        .width('100%')
        .borderRadius(OrbitTokens.RADIUS_LG)
        .clip(true)
        .attributeModifier(
          orbitSurface(
            this.store.palette.listCard,
            this.store.palette.hairline,
            OrbitTokens.HAIRLINE,
            ShadowStyle.OUTER_DEFAULT_MD
          )
        )
      }
    }
  }

  @Builder
  private renderQueuedPromptsTray() {
    if (this.store.activeQueuedPrompts.length > 0) {
      Column({ space: OrbitTokens.SPACE_XS }) {
        Row() {
          Text(`Queued (${this.store.activeQueuedPrompts.length})`)
            .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium))
          Blank()
        }
        .width('100%')
        .padding({ left: OrbitTokens.SPACE_XS, right: OrbitTokens.SPACE_XS })

        ForEach(
          this.store.activeQueuedPrompts,
          (qItem: QueuedPromptItem) => {
            Row({ space: OrbitTokens.SPACE_SM }) {
              Text(
                qItem.displayPrompt.length > 0
                  ? qItem.displayPrompt
                  : qItem.badges.map((b: PromptBadgeDto) => b.label).join(' ')
              )
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                .maxLines(1)
                .textOverflow({ overflow: TextOverflow.Ellipsis })
                .layoutWeight(1)

              Row({ space: OrbitTokens.SPACE_XS }) {
                Button('Edit', { type: ButtonType.Capsule, stateEffect: true })
                  .attributeModifier(
                    orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface)
                  )
                  .height(34)
                  .padding({ left: 10, right: 10 })
                  .onClick(() => {
                    this.store.editQueuedPrompt(qItem.id);
                  })

                Button('Send', { type: ButtonType.Capsule, stateEffect: true })
                  .attributeModifier(
                    orbitButtonMeta(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
                  )
                  .height(34)
                  .padding({ left: 10, right: 10 })
                  .onClick(() => {
                    this.store.sendQueuedPromptNow(qItem.id);
                  })

                Button({ type: ButtonType.Circle, stateEffect: true }) {
                  SymbolGlyph($r('sys.symbol.xmark'))
                    .fontSize(13)
                    .renderingStrategy(SymbolRenderingStrategy.SINGLE)
                    .fontColor([this.store.palette.iconSecondary])
                }
                .width(36)
                .height(36)
                .padding(0)
                .backgroundColor(Color.Transparent)
                .accessibilityText('Remove queued prompt')
                .onClick(() => {
                  this.store.removeQueuedPrompt(qItem.id);
                })
              }
            }
            .width('100%')
            .padding({
              left: OrbitTokens.SPACE_MD,
              right: OrbitTokens.SPACE_XS,
              top: 4,
              bottom: 4
            })
            .backgroundColor(this.store.palette.listCard)
            .borderRadius(OrbitTokens.RADIUS_MD)
            .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
            .alignItems(VerticalAlign.Center)
          },
          (qItem: QueuedPromptItem) => qItem.id
        )
      }
      .width('100%')
    }
  }

  @Builder
  private renderAttachmentAndContextStrip() {
    if (
      this.store.isStagingAttachment ||
      this.store.stagedAttachments.length > 0 ||
      this.store.composerBadges.length > 0 ||
      this.store.quotedSnippets.length > 0
    ) {
      Scroll() {
        Row({ space: OrbitTokens.SPACE_XS }) {
          if (this.store.isStagingAttachment) {
            Row() {
              LoadingProgress()
                .width(18)
                .height(18)
                .color(this.store.palette.iconSecondary)
            }
            .width(OrbitTokens.HIT_COMPACT)
            .height(OrbitTokens.HIT_COMPACT)
            .justifyContent(FlexAlign.Center)
            .borderRadius(OrbitTokens.RADIUS_MD)
            .backgroundColor(this.store.palette.subtleSurface)
          }

          ForEach(
            this.store.stagedAttachments,
            (att: ComposerAttachmentItem) => {
              if (att.isImage && att.localUri.length > 0) {
                Stack({ alignContent: Alignment.TopEnd }) {
                  Image(att.localUri)
                    .width(52)
                    .height(52)
                    .objectFit(ImageFit.Cover)
                    .borderRadius(OrbitTokens.RADIUS_MD)
                    .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
                  Button({ type: ButtonType.Circle, stateEffect: true }) {
                    SymbolGlyph($r('sys.symbol.xmark'))
                      .fontSize(10)
                      .renderingStrategy(SymbolRenderingStrategy.SINGLE)
                      .fontColor([$r('sys.color.font_on_primary')])
                  }
                  .width(22)
                  .height(22)
                  .padding(0)
                  .margin(2)
                  .backgroundColor(this.store.palette.overlayPillBg)
                  .accessibilityText(`Remove ${att.filename}`)
                  .onClick(() => {
                    this.store.removeStagedAttachment(att.id);
                  })
                }
                .width(52)
                .height(52)
              } else {
                Row({ space: 4 }) {
                  SymbolGlyph(resolveSemanticFileSymbol(att.filename))
                    .fontSize(14)
                    .fontColor([this.store.palette.iconSecondary])
                  Text(att.filename)
                    .attributeModifier(orbitTextMeta(this.store.palette.fgPrimary, FontWeight.Medium))
                    .maxLines(1)
                    .textOverflow({ overflow: TextOverflow.Ellipsis })
                    .constraintSize({ maxWidth: 140 })
                  Button({ type: ButtonType.Circle, stateEffect: true }) {
                    SymbolGlyph($r('sys.symbol.xmark'))
                      .fontSize(11)
                      .renderingStrategy(SymbolRenderingStrategy.SINGLE)
                      .fontColor([this.store.palette.iconSecondary])
                  }
                  .width(28)
                  .height(28)
                  .padding(0)
                  .backgroundColor(Color.Transparent)
                  .accessibilityText(`Remove ${att.filename}`)
                  .onClick(() => {
                    this.store.removeStagedAttachment(att.id);
                  })
                }
                .constraintSize({ minHeight: 34 })
                .padding({ left: 10, right: 4 })
                .borderRadius(OrbitTokens.RADIUS_SM)
                .backgroundColor(this.store.palette.subtleSurface)
                .alignItems(VerticalAlign.Center)
              }
            },
            (att: ComposerAttachmentItem) => att.id
          )

          ForEach(
            this.store.composerBadges,
            (badge: PromptBadgeDto) => {
              Row({ space: 4 }) {
                SymbolGlyph(this.badgeSymbol(badge.kind, badge.label))
                  .fontSize(14)
                  .fontColor([this.store.palette.iconSecondary])
                Text(badge.label)
                  .attributeModifier(orbitTextMeta(this.store.palette.fgPrimary, FontWeight.Medium))
                  .maxLines(1)
                Button({ type: ButtonType.Circle, stateEffect: true }) {
                  SymbolGlyph($r('sys.symbol.xmark'))
                    .fontSize(11)
                    .renderingStrategy(SymbolRenderingStrategy.SINGLE)
                    .fontColor([this.store.palette.iconSecondary])
                }
                .width(28)
                .height(28)
                .padding(0)
                .backgroundColor(Color.Transparent)
                .accessibilityText(`Remove ${badge.label}`)
                .onClick(() => {
                  this.store.removeComposerBadge(badge.label);
                })
              }
              .constraintSize({ minHeight: 34 })
              .padding({ left: 10, right: 4 })
              .borderRadius(OrbitTokens.RADIUS_SM)
              .backgroundColor(this.store.palette.subtleSurface)
              .alignItems(VerticalAlign.Center)
            },
            (badge: PromptBadgeDto) => `${badge.kind}_${badge.label}`
          )

          ForEach(
            this.store.quotedSnippets,
            (snip: QuotedDiffSnippet) => {
              Row({ space: 4 }) {
                SymbolGlyph($r('sys.symbol.quote'))
                  .fontSize(13)
                  .fontColor([this.store.palette.iconSecondary])
                Text(snip.lineLabel)
                  .attributeModifier(orbitTextMeta(this.store.palette.fgPrimary, FontWeight.Medium))
                  .maxLines(1)
                Button({ type: ButtonType.Circle, stateEffect: true }) {
                  SymbolGlyph($r('sys.symbol.xmark'))
                    .fontSize(11)
                    .renderingStrategy(SymbolRenderingStrategy.SINGLE)
                    .fontColor([this.store.palette.iconSecondary])
                }
                .width(28)
                .height(28)
                .padding(0)
                .backgroundColor(Color.Transparent)
                .accessibilityText(`Remove ${snip.lineLabel}`)
                .onClick(() => {
                  this.store.removeQuotedSnippet(snip.id);
                })
              }
              .constraintSize({ minHeight: 34 })
              .padding({ left: 10, right: 4 })
              .borderRadius(OrbitTokens.RADIUS_SM)
              .backgroundColor(this.store.palette.subtleSurface)
              .alignItems(VerticalAlign.Center)
            },
            (snip: QuotedDiffSnippet) => snip.id
          )
        }
        .padding({
          left: OrbitTokens.SPACE_MD,
          right: OrbitTokens.SPACE_MD,
          top: OrbitTokens.SPACE_SM
        })
      }
      .scrollable(ScrollDirection.Horizontal)
      .scrollBar(BarState.Off)
      .width('100%')
    }
  }

  private completedPlanCount(): number {
    let done = 0;
    for (let i = 0; i < this.store.stagePlanSteps.length; i++) {
      if (this.store.stagePlanSteps[i].status === 'completed') {
        done += 1;
      }
    }
    return done;
  }

  private formatVoiceClock(sec: number): string {
    const mins = Math.floor(sec / 60);
    const rem = sec % 60;
    return `${mins}:${rem.toString().padStart(2, '0')}`;
  }

  private planStepSymbol(status: string): Resource {
    if (status === 'completed') {
      return $r('sys.symbol.checkmark_circle_fill');
    }
    if (status === 'failed') {
      return $r('sys.symbol.xmark_circle');
    }
    if (status === 'in_progress') {
      return $r('sys.symbol.dot_circle');
    }
    return $r('sys.symbol.circle');
  }

  private planStepColor(status: string): ResourceColor {
    if (status === 'completed') {
      return this.store.palette.statusDirect;
    }
    if (status === 'failed') {
      return this.store.palette.statusDanger;
    }
    if (status === 'in_progress') {
      return this.store.palette.accentPrimary;
    }
    return this.store.palette.fgMuted;
  }

  @Builder
  private renderPlanProgressShelf() {
    if (
      this.store.stagePlanSteps.length > 0 &&
      this.store.stagePendingPlanApproval === null &&
      this.store.pendingPermissions.length === 0 &&
      this.store.pendingQuestions.length === 0
    ) {
      Column({ space: OrbitTokens.SPACE_XS }) {
        Row({ space: OrbitTokens.SPACE_SM }) {
          SymbolGlyph($r('sys.symbol.list_checkmark'))
            .fontSize(14)
            .fontColor([this.store.palette.iconSecondary])

          Text($r('app.string.composer_plan_progress'))
            .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium))
            .layoutWeight(1)

          Text(`${this.completedPlanCount()}/${this.store.stagePlanSteps.length}`)
            .attributeModifier(orbitTextMono(this.store.palette.fgMuted, FontWeight.Medium))

          SymbolGlyph(
            this.planShelfExpanded ? $r('sys.symbol.chevron_down') : $r('sys.symbol.chevron_up')
          )
            .fontSize(12)
            .fontColor([this.store.palette.fgMuted])
        }
        .width('100%')
        .constraintSize({ minHeight: 34 })
        .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
        .alignItems(VerticalAlign.Center)
        .onClick(() => {
          this.getUIContext().animateTo({ duration: OrbitTokens.DUR_UI, curve: Curve.FastOutSlowIn }, () => {
            this.planShelfExpanded = !this.planShelfExpanded;
          });
        })

        if (this.planShelfExpanded) {
          Column({ space: 6 }) {
            ForEach(
              this.store.stagePlanSteps,
              (step: SessionPlanStepDto) => {
                Row({ space: OrbitTokens.SPACE_SM }) {
                  SymbolGlyph(this.planStepSymbol(step.status))
                    .fontSize(13)
                    .fontColor([this.planStepColor(step.status)])
                  Text(
                    step.status === 'in_progress' &&
                      step.active_form !== undefined &&
                      step.active_form !== null &&
                      step.active_form.length > 0
                      ? step.active_form
                      : step.title
                  )
                    .attributeModifier(
                      orbitTextMeta(
                        step.status === 'completed'
                          ? this.store.palette.fgMuted
                          : this.store.palette.fgPrimary,
                        step.status === 'in_progress' ? FontWeight.Medium : FontWeight.Normal
                      )
                    )
                    .maxLines(1)
                    .textOverflow({ overflow: TextOverflow.Ellipsis })
                    .layoutWeight(1)
                }
                .width('100%')
                .alignItems(VerticalAlign.Center)
              },
              (step: SessionPlanStepDto): string => `plan_${step.id}_${step.status}_${step.updated_seq}`
            )
          }
          .width('100%')
          .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD, bottom: OrbitTokens.SPACE_SM })
        }
      }
      .width('100%')
      .borderRadius(OrbitTokens.RADIUS_LG)
      .attributeModifier(OrbitTheme.dockedCard(this.store.palette))
    }
  }

  @Builder
  private renderInlineVoiceStrip() {
    if (this.store.isVoiceRecording) {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Row()
          .width(8)
          .height(8)
          .borderRadius(4)
          .backgroundColor(this.store.palette.statusDanger)

        Text(
          this.store.voiceInterimTranscript.length > 0
            ? this.store.voiceInterimTranscript
            : $r('app.string.composer_voice_listening')
        )
          .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Normal))
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .layoutWeight(1)

        Text(this.formatVoiceClock(this.store.voiceElapsedSeconds))
          .attributeModifier(orbitTextMono(this.store.palette.fgMuted))

        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.xmark'))
            .fontSize(13)
            .renderingStrategy(SymbolRenderingStrategy.SINGLE)
            .fontColor([this.store.palette.iconSecondary])
        }
        .width(32)
        .height(32)
        .padding(0)
        .backgroundColor(this.store.palette.subtleSurface)
        .accessibilityText($r('app.string.composer_voice_cancel'))
        .onClick(() => {
          this.store.cancelVoiceDictation();
        })
      }
      .width('100%')
      .padding({
        left: OrbitTokens.SPACE_MD,
        right: OrbitTokens.SPACE_SM,
        top: OrbitTokens.SPACE_SM,
        bottom: OrbitTokens.SPACE_XS
      })
      .alignItems(VerticalAlign.Center)
    }
  }

  private inputPlaceholder(): ResourceStr {
    if (this.store.activeSlashDirective.length > 0) {
      return '';
    }
    return $r('app.string.composer_placeholder');
  }

  build() {
    Column({ space: OrbitTokens.SPACE_SM }) {
      this.renderAutocompletePopover()
      this.renderPlanProgressShelf()
      this.renderQueuedPromptsTray()

      // Main Orbital Quiet Precision Docked Composer Card
      Column() {
        this.renderInlineVoiceStrip()
        this.renderAttachmentAndContextStrip()

        Row({ space: OrbitTokens.SPACE_XS }) {
          if (this.store.activeSlashDirective.length > 0) {
            Button(this.store.activeSlashDirective, { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(
                orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface)
              )
              .fontFamily(OrbitTokens.FAMILY_MONO)
              .height(32)
              .padding({ left: 10, right: 10 })
              .margin({ left: OrbitTokens.SPACE_MD })
              .onClick(() => {
                this.store.clearSlashDirective();
              })
          }

          TextArea({
            placeholder: this.inputPlaceholder(),
            text: this.store.composerText
          })
            .attributeModifier(
              orbitComposerArea(this.store.palette.fgPrimary, this.store.palette.fgMuted)
            )
            .layoutWeight(1)
            .constraintSize({ minHeight: 38, maxHeight: 132 })
            .padding({
              left: this.store.activeSlashDirective.length > 0 ? OrbitTokens.SPACE_XS : OrbitTokens.SPACE_LG,
              right: OrbitTokens.SPACE_LG,
              top: 10,
              bottom: 2
            })
            .onChange((val: string) => {
              if (val !== this.store.composerText) {
                this.dismissedTriggerToken = '';
              }
              this.store.composerText = val;
              if (val.indexOf('@') >= 0) {
                this.store.ensureWorkspaceFilesLoaded();
              }
            })
            .onKeyEvent((event: KeyEvent) => {
              if (
                event.type === KeyType.Down &&
                event.keyCode === KeyCode.KEYCODE_DEL &&
                this.store.composerText.length === 0 &&
                this.store.activeSlashDirective.length > 0
              ) {
                this.store.clearSlashDirective();
              }
            })
        }
        .width('100%')
        .alignItems(VerticalAlign.Center)

        // Bottom Control Bar: native Buttons (>= 40vp hit targets), only Send/Queue/Stop carries fill
        Row({ space: 0 }) {
          Button({ type: ButtonType.Circle, stateEffect: true }) {
            SymbolGlyph($r('sys.symbol.plus'))
              .fontSize(18)
              .renderingStrategy(SymbolRenderingStrategy.SINGLE)
              .fontColor([
                this.store.selectedHostId.length > 0
                  ? this.store.palette.iconSecondary
                  : this.store.palette.fgMuted
              ])
          }
          .width(OrbitTokens.HIT_COMPACT)
          .height(OrbitTokens.HIT_COMPACT)
          .padding(0)
          .backgroundColor(Color.Transparent)
          .accessibilityText('Attach photos, camera, or files')
          .bindMenu(this.renderPlusAttachmentMenu())

          // Model + Effort Trigger with Fixed-Width Ghost Anchor for Stable Popover Alignment
          Stack({ alignContent: Alignment.Start }) {
            Row()
              .width(148)
              .height(OrbitTokens.HIT_COMPACT)
              .hitTestBehavior(HitTestMode.None)
              .bindPopup(this.store.showModelPickerSheet, {
                builder: this.buildModelPickerSheet(),
                placement: Placement.Top,
                enableArrow: true,
                arrowPointPosition: ArrowPointPosition.CENTER,
                targetSpace: 8,
                radius: 20,
                shadow: ShadowStyle.OUTER_DEFAULT_MD,
                backgroundBlurStyle: BlurStyle.COMPONENT_ULTRA_THICK,
                popupColor: this.store.palette.listCard,
                mask: true,
                autoCancel: true,
                focusable: true,
                onStateChange: (event) => {
                  if (!event.isVisible) {
                    this.store.showModelPickerSheet = false;
                  }
                }
              })

            Button({ type: ButtonType.Normal, stateEffect: true }) {
              Row({ space: 6 }) {
                OrbitProviderBrandIcon({ provider: this.store.composerProvider, iconSize: 16 })
                Text(this.store.currentModelDisplayLabel())
                  .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Normal))
                  .maxLines(1)
                  .textOverflow({ overflow: TextOverflow.Ellipsis })
                  .flexShrink(1)
                if (this.store.currentEffortDisplayLabel().length > 0) {
                  Text(this.store.currentEffortDisplayLabel())
                    .attributeModifier(orbitTextSubhead(this.store.palette.fgMuted, FontWeight.Normal))
                    .maxLines(1)
                    .flexShrink(0)
                }
                SymbolGlyph($r('sys.symbol.chevron_down'))
                  .fontSize(11)
                  .fontColor([this.store.palette.fgMuted])
              }
              .alignItems(VerticalAlign.Center)
            }
            .height(OrbitTokens.HIT_COMPACT)
            .constraintSize({ maxWidth: '100%' })
            .padding({ left: 2, right: 8 })
            .borderRadius(OrbitTokens.RADIUS_MD)
            .backgroundColor(Color.Transparent)
            .accessibilityText(`Model: ${this.store.currentModelPillLabel()}`)
            .onClick(() => {
              dismissHarmonyKeyboard(this.getUIContext());
              this.store.triggerHaptic(6);
              this.store.showModelPickerSheet = !this.store.showModelPickerSheet;
              if (
                this.store.showModelPickerSheet &&
                this.store.modelsForProvider(this.store.composerProvider).length === 0
              ) {
                this.store.refreshModelsAndQuotas(false);
              }
            })
          }
          .height(OrbitTokens.HIT_COMPACT)
          .layoutWeight(1)

          // Fast Mode Toggle Button
          Button({ type: ButtonType.Circle, stateEffect: true }) {
            SymbolGlyph($r('sys.symbol.bolt'))
              .fontSize(16)
              .renderingStrategy(SymbolRenderingStrategy.SINGLE)
              .fontColor([
                this.store.composerFastMode
                  ? this.store.palette.accentPrimary
                  : this.store.palette.iconSecondary
              ])
          }
          .width(OrbitTokens.HIT_COMPACT)
          .height(OrbitTokens.HIT_COMPACT)
          .padding(0)
          .backgroundColor(
            this.store.composerFastMode
              ? this.store.palette.accentPrimarySoft
              : Color.Transparent
          )
          .accessibilityText(this.store.composerFastMode ? 'Fast Mode on' : 'Fast Mode off')
          .onClick(() => {
            this.store.toggleFastMode();
          })

          // Access Permissions Shield Button + Native Menu
          Button({ type: ButtonType.Circle, stateEffect: true }) {
            Image(this.permissionModeIcon(this.store.composerMode))
              .width(17)
              .height(17)
              .objectFit(ImageFit.Contain)
              .interpolation(ImageInterpolation.High)
              .draggable(false)
              .colorFilter(orbitTintColorFilter(this.store.palette.tintMutedHex))
          }
          .width(OrbitTokens.HIT_COMPACT)
          .height(OrbitTokens.HIT_COMPACT)
          .padding(0)
          .backgroundColor(Color.Transparent)
          .accessibilityText(`Access Permissions: ${formatModeLabel(normalizeModeSlug(this.store.composerMode))}`)
          .bindMenu(this.renderPermissionModeMenu())

          // SINGLE Stationary Right Action Circle (Codex-Style Voice: empty+idle -> Mic, text/attachment -> Send)
          if (this.store.isVoiceRecording) {
            Button({ type: ButtonType.Circle, stateEffect: true }) {
              SymbolGlyph($r('sys.symbol.checkmark'))
                .fontSize(16)
                .renderingStrategy(SymbolRenderingStrategy.SINGLE)
                .fontColor([this.store.palette.onAccent])
            }
            .width(OrbitTokens.HIT_COMPACT)
            .height(OrbitTokens.HIT_COMPACT)
            .padding(0)
            .backgroundColor(this.store.palette.accentPrimaryFill)
            .accessibilityText($r('app.string.composer_voice_stop'))
            .onClick(() => {
              this.store.finishVoiceDictation();
            })
          } else if (this.isAgentRunning()) {
            if (this.canSend()) {
              Row({ space: 2 }) {
                Button({ type: ButtonType.Capsule, stateEffect: true }) {
                  Row({ space: 4 }) {
                    SymbolGlyph($r('sys.symbol.stop_circle_fill'))
                      .fontSize(15)
                      .fontColor([this.store.palette.statusDanger])
                    Text($r('app.string.action_stop'))
                      .attributeModifier(orbitTextMeta(this.store.palette.statusDanger, FontWeight.Medium))
                  }
                }
                .height(OrbitTokens.HIT_COMPACT)
                .padding({ left: 8, right: 8 })
                .backgroundColor(Color.Transparent)
                .accessibilityText('Stop agent')
                .onClick(() => {
                  this.store.interruptCurrentAgent();
                })

                Button({ type: ButtonType.Circle, stateEffect: true }) {
                  SymbolGlyph($r('sys.symbol.arrow_up_to_line'))
                    .fontSize(16)
                    .renderingStrategy(SymbolRenderingStrategy.SINGLE)
                    .fontColor([this.store.palette.onAccent])
                }
                .width(OrbitTokens.HIT_COMPACT)
                .height(OrbitTokens.HIT_COMPACT)
                .padding(0)
                .backgroundColor(this.store.palette.accentPrimaryFill)
                .accessibilityText('Queue prompt')
                .onClick(() => {
                  this.store.enqueueCurrentDraft();
                })
              }
            } else {
              Button({ type: ButtonType.Circle, stateEffect: true }) {
                Row()
                  .width(12)
                  .height(12)
                  .borderRadius(OrbitTokens.RADIUS_XS)
                  .backgroundColor(this.store.palette.statusDanger)
              }
              .width(OrbitTokens.HIT_COMPACT)
              .height(OrbitTokens.HIT_COMPACT)
              .padding(0)
              .backgroundColor(this.store.palette.subtleSurface)
              .accessibilityText('Stop agent')
              .onClick(() => {
                this.store.interruptCurrentAgent();
              })
            }
          } else if (this.canSend()) {
            Button({ type: ButtonType.Circle, stateEffect: true }) {
              SymbolGlyph($r('sys.symbol.arrow_up'))
                .fontSize(16)
                .renderingStrategy(SymbolRenderingStrategy.SINGLE)
                .fontColor([this.store.palette.onAccent])
            }
            .width(OrbitTokens.HIT_COMPACT)
            .height(OrbitTokens.HIT_COMPACT)
            .padding(0)
            .backgroundColor(this.store.palette.accentPrimaryFill)
            .accessibilityText($r('app.string.action_send'))
            .onClick(() => {
              this.store.submitComposerPrompt();
            })
          } else {
            Button({ type: ButtonType.Circle, stateEffect: true }) {
              SymbolGlyph($r('sys.symbol.mic'))
                .fontSize(17)
                .renderingStrategy(SymbolRenderingStrategy.SINGLE)
                .fontColor([this.store.palette.iconPrimary])
            }
            .width(OrbitTokens.HIT_COMPACT)
            .height(OrbitTokens.HIT_COMPACT)
            .padding(0)
            .backgroundColor(this.store.palette.subtleSurface)
            .accessibilityText($r('app.string.composer_voice_start'))
            .onClick(() => {
              dismissHarmonyKeyboard(this.getUIContext());
              this.store.startVoiceDictation(this.getUIContext().getHostContext());
            })
          }
        }
        .width('100%')
        .padding({ left: 6, right: 6, bottom: 6 })
        .alignItems(VerticalAlign.Center)
      }
      .width('100%')
      .constraintSize({ maxWidth: OrbitTokens.STAGE_MAX_READING_WIDTH })
      .borderRadius(OrbitTokens.RADIUS_XXL)
      .attributeModifier(OrbitTheme.dockedCard(this.store.palette))
    }
    .width('100%')
    .alignItems(HorizontalAlign.Center)
    .padding({
      left: OrbitTokens.SPACE_LG,
      right: OrbitTokens.SPACE_LG,
      top: OrbitTokens.SPACE_SM,
      bottom: this.store.keyboardHeightVp > 0
        ? OrbitTokens.SPACE_SM
        : Math.max(OrbitTokens.SPACE_SM, this.store.safeBottomVp)
    })
  }
}
