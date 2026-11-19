import {
  ComposerMentionCandidate,
  GitFileEntryDto,
  PromptBadgeDto,
  QueuedPromptItem,
  QuotedDiffSnippet,
  SlashCommandItem,
  basenameOfPath
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import {
  OrbitTokens,
  orbitComposerArea,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead
} from '../../theme/OrbitTokens';

interface ScoredFileCandidate {
  score: number;
  item: ComposerMentionCandidate;
}

interface ParsedMentionTrigger {
  query: string;
  strippedText: string;
  rawToken: string;
}

interface ParsedSlashTrigger {
  query: string;
  strippedText: string;
  rawToken: string;
}

@ComponentV2
export struct OmniComposer {
  @Local dismissedTriggerToken: string = '';

  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  private isAgentRunning(): boolean {
    return this.store.isActiveAgentRunning();
  }

  private canSend(): boolean {
    return this.store.hasComposerDraftContent();
  }

  private isWhitespaceChar(ch: string): boolean {
    return ch === ' ' || ch === '\n' || ch === '\t' || ch === '\r';
  }

  private parseSlashTrigger(): ParsedSlashTrigger | null {
    const text = this.store.composerText;
    if (!text.startsWith('/')) {
      return null;
    }
    let firstSpace = -1;
    for (let i = 0; i < text.length; i++) {
      if (this.isWhitespaceChar(text.charAt(i))) {
        firstSpace = i;
        break;
      }
    }
    // Only show slash popover while actively typing the leading /<query> token
    if (firstSpace >= 0) {
      return null;
    }
    const rawToken = text;
    if (this.dismissedTriggerToken.length > 0 && this.dismissedTriggerToken === rawToken) {
      return null;
    }
    return {
      query: text.substring(1).toLowerCase(),
      strippedText: '',
      rawToken: rawToken
    };
  }

  private filteredSlashCommands(): SlashCommandItem[] {
    const trig = this.parseSlashTrigger();
    if (trig === null) {
      return [];
    }
    const q = trig.query;
    const all = this.store.slashCommands;
    const out: SlashCommandItem[] = [];
    for (let i = 0; i < all.length; i++) {
      const cmd = all[i];
      const slug = cmd.command.substring(1).toLowerCase();
      if (
        q.length === 0 ||
        slug.indexOf(q) >= 0 ||
        cmd.title.toLowerCase().indexOf(q) >= 0 ||
        cmd.description.toLowerCase().indexOf(q) >= 0
      ) {
        out.push(cmd);
      }
    }
    return out;
  }

  private parseTrailingMention(): ParsedMentionTrigger | null {
    const text = this.store.composerText;
    const lastAt = text.lastIndexOf('@');
    if (lastAt < 0) {
      return null;
    }
    if (lastAt > 0 && !this.isWhitespaceChar(text.charAt(lastAt - 1))) {
      return null;
    }
    const query = text.substring(lastAt + 1);
    for (let i = 0; i < query.length; i++) {
      if (this.isWhitespaceChar(query.charAt(i))) {
        return null;
      }
    }
    const rawToken = `@${query}`;
    if (this.dismissedTriggerToken.length > 0 && this.dismissedTriggerToken === rawToken) {
      return null;
    }
    const stripped = text.substring(0, lastAt).replace(/[ \t]+$/, '');
    return {
      query: query.toLowerCase(),
      strippedText: stripped,
      rawToken: rawToken
    };
  }

  private fileMatchScore(relPath: string, base: string, q: string): number {
    if (q.length === 0) {
      return 10;
    }
    const baseLower = base.toLowerCase();
    const fullLower = relPath.toLowerCase();
    if (baseLower === q) {
      return 120;
    }
    if (baseLower.startsWith(q)) {
      return 100;
    }
    if (baseLower.indexOf(q) >= 0) {
      return 82;
    }
    if (fullLower.indexOf(q) >= 0) {
      return 64;
    }
    let qi = 0;
    for (let i = 0; i < fullLower.length && qi < q.length; i++) {
      if (fullLower.charAt(i) === q.charAt(qi)) {
        qi++;
      }
    }
    return qi === q.length ? 40 : 0;
  }

  private mentionCandidates(): ComposerMentionCandidate[] {
    const trig = this.parseTrailingMention();
    if (trig === null) {
      return [];
    }
    const q = trig.query;
    const results: ComposerMentionCandidate[] = [];

    const builtins: ComposerMentionCandidate[] = [
      {
        key: 'special:diff',
        kind: 'diff',
        tokenLabel: '@diff',
        title: 'Working Tree Diff',
        subtitle: 'Uncommitted changes',
        filePath: '',
        gitStatus: ''
      },
      {
        key: 'special:staged',
        kind: 'staged',
        tokenLabel: '@staged',
        title: 'Staged Diff',
        subtitle: 'Git index staged changes',
        filePath: '',
        gitStatus: ''
      },
      {
        key: 'special:terminal',
        kind: 'terminal',
        tokenLabel: '@terminal',
        title: 'Terminal Output',
        subtitle: 'Recent active PTY lines',
        filePath: '',
        gitStatus: ''
      }
    ];

    for (let i = 0; i < builtins.length; i++) {
      const b = builtins[i];
      const slug = b.tokenLabel.substring(1).toLowerCase();
      if (
        q.length === 0 ||
        slug.indexOf(q) >= 0 ||
        b.title.toLowerCase().indexOf(q) >= 0 ||
        b.subtitle.toLowerCase().indexOf(q) >= 0 ||
        (b.kind === 'terminal' && 'pty'.indexOf(q) >= 0)
      ) {
        results.push(b);
      }
    }

    const gitStatusMap: Map<string, string> = new Map<string, string>();
    const allPaths: string[] = [];
    const seenPaths: Set<string> = new Set<string>();
    const stFiles: GitFileEntryDto[] = this.store.gitStatus?.files ?? [];
    for (let i = 0; i < stFiles.length; i++) {
      const gf = stFiles[i];
      gitStatusMap.set(gf.path, gf.untracked ? 'U' : gf.status.length > 0 ? gf.status : 'M');
      if (!seenPaths.has(gf.path)) {
        seenPaths.add(gf.path);
        allPaths.push(gf.path);
      }
    }
    for (let i = 0; i < this.store.workspaceFiles.length; i++) {
      const fp = this.store.workspaceFiles[i];
      if (!seenPaths.has(fp)) {
        seenPaths.add(fp);
        allPaths.push(fp);
      }
    }

    const scored: ScoredFileCandidate[] = [];
    for (let i = 0; i < allPaths.length; i++) {
      const path = allPaths[i];
      const base = basenameOfPath(path);
      const baseScore = this.fileMatchScore(path, base, q);
      if (baseScore <= 0) {
        continue;
      }
      const statusBadge = gitStatusMap.get(path) ?? '';
      const slashIdx = path.lastIndexOf('/');
      const dir = slashIdx > 0 ? path.substring(0, slashIdx) : '';
      const totalScore = baseScore + (statusBadge.length > 0 ? 18 : 0);
      scored.push({
        score: totalScore,
        item: {
          key: `file:${path}`,
          kind: 'file',
          tokenLabel: `@${base}`,
          title: base,
          subtitle: dir,
          filePath: path,
          gitStatus: statusBadge
        }
      });
    }

    scored.sort((a: ScoredFileCandidate, b: ScoredFileCandidate) => b.score - a.score);
    for (let i = 0; i < scored.length && results.length < 6; i++) {
      results.push(scored[i].item);
    }
    return results.slice(0, 6);
  }

  private providerDotColor(): string {
    const prov = this.store.composerProvider.trim().toLowerCase();
    if (prov === 'codex') {
      return this.store.palette.statusDirect;
    }
    if (prov === 'claude_code' || prov === 'claude') {
      return this.store.palette.statusRelay;
    }
    return this.store.palette.accentPrimary;
  }

  private modeToneColor(): string {
    const mode = this.store.composerMode;
    if (mode === 'full_access') {
      return this.store.palette.statusWarning;
    }
    if (mode === 'accept_edits') {
      return this.store.palette.statusDirect;
    }
    if (mode === 'plan') {
      return this.store.palette.accentPrimary;
    }
    return this.store.palette.fgSecondary;
  }

  private hasAttachedChips(): boolean {
    return (
      this.store.composerBadges.length > 0 ||
      this.store.quotedSnippets.length > 0 ||
      this.store.worktreeBranch.trim().length > 0
    );
  }

  @Builder
  private renderContextChipsTray() {
    if (this.hasAttachedChips()) {
      Scroll() {
        Row({ space: OrbitTokens.SPACE_SM }) {
          if (this.store.worktreeBranch.trim().length > 0) {
            Row({ space: OrbitTokens.SPACE_XS }) {
              Text(`⎇ ${this.store.worktreeBranch.trim()}`)
                .attributeModifier(
                  orbitTextMono(this.store.palette.statusRelay, FontWeight.Medium)
                )
              Text('×')
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Bold))
            }
            .height(28)
            .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
            .backgroundColor(this.store.palette.statusRelaySoft)
            .borderRadius(OrbitTokens.PILL_RADIUS)
            .accessibilityText(`Remove worktree branch ${this.store.worktreeBranch.trim()}`)
            .onClick(() => {
              this.store.worktreeBranch = '';
            })
          }

          ForEach(
            this.store.composerBadges,
            (badge: PromptBadgeDto) => {
              Row({ space: OrbitTokens.SPACE_XS }) {
                Text(badge.label)
                  .attributeModifier(
                    orbitTextMono(this.store.palette.accentPrimary, FontWeight.Medium)
                  )
                Text('×')
                  .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Bold))
              }
              .height(28)
              .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
              .backgroundColor(this.store.palette.accentPrimarySoft)
              .borderRadius(OrbitTokens.PILL_RADIUS)
              .accessibilityText(`Remove context ${badge.label}`)
              .onClick(() => {
                this.store.removeComposerBadge(badge.label);
              })
            },
            (badge: PromptBadgeDto) => badge.label
          )

          ForEach(
            this.store.quotedSnippets,
            (snip: QuotedDiffSnippet) => {
              Row({ space: OrbitTokens.SPACE_XS }) {
                Text(`❝ ${snip.lineLabel}`)
                  .attributeModifier(
                    orbitTextMono(this.store.palette.statusDirect, FontWeight.Medium)
                  )
                Text('×')
                  .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Bold))
              }
              .height(28)
              .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
              .backgroundColor(this.store.palette.statusDirectSoft)
              .borderRadius(OrbitTokens.PILL_RADIUS)
              .accessibilityText(`Remove quoted snippet ${snip.lineLabel}`)
              .onClick(() => {
                this.store.removeQuotedSnippet(snip.id);
              })
            },
            (snip: QuotedDiffSnippet) => snip.id
          )
        }
      }
      .scrollable(ScrollDirection.Horizontal)
      .scrollBar(BarState.Off)
      .width('100%')
      .padding({
        left: OrbitTokens.SPACE_MD,
        right: OrbitTokens.SPACE_MD,
        top: OrbitTokens.SPACE_SM
      })
    }
  }

  @Builder
  private renderInlineSlashPopover() {
    if (this.filteredSlashCommands().length > 0) {
      Column({ space: OrbitTokens.SPACE_XS }) {
        Row() {
          Text('SLASH DIRECTIVES')
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Bold))
          Blank()
          Stack({ alignContent: Alignment.Center }) {
            Text('×')
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Bold))
          }
          .width(OrbitTokens.HIT_MIN)
          .height(OrbitTokens.HIT_MIN)
          .accessibilityText('Dismiss slash directives popover')
          .onClick(() => {
            const trig = this.parseSlashTrigger();
            if (trig !== null) {
              this.dismissedTriggerToken = trig.rawToken;
            }
          })
        }
        .width('100%')
        .padding({ left: OrbitTokens.SPACE_SM })
        .alignItems(VerticalAlign.Center)

        ForEach(
          this.filteredSlashCommands(),
          (cmd: SlashCommandItem) => {
            Row({ space: OrbitTokens.SPACE_SM }) {
              Text(cmd.command)
                .attributeModifier(orbitTextMono(this.store.palette.accentPrimary, FontWeight.Bold))
                .width(72)

              Text(cmd.description)
                .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                .maxLines(1)
                .textOverflow({ overflow: TextOverflow.Ellipsis })
                .layoutWeight(1)
            }
            .width('100%')
            .height(36)
            .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
            .borderRadius(OrbitTokens.PILL_RADIUS)
            .alignItems(VerticalAlign.Center)
            .onClick(() => {
              const trig = this.parseSlashTrigger();
              this.store.applyInlineSlashCommand(cmd, trig !== null ? trig.strippedText : '');
              this.dismissedTriggerToken = '';
            })
          },
          (cmd: SlashCommandItem) => cmd.command
        )
      }
      .width('100%')
      .padding(OrbitTokens.SPACE_SM)
      .backgroundColor(this.store.palette.elevated)
      .borderRadius(OrbitTokens.CARD_RADIUS)
      .border({
        width: OrbitTokens.HAIRLINE,
        color: this.store.palette.hairline
      })
      .shadow({
        radius: 12,
        color: '#22000000',
        offsetX: 0,
        offsetY: -2
      })
    }
  }

  @Builder
  private renderInlineMentionPopover() {
    if (this.parseSlashTrigger() === null && this.mentionCandidates().length > 0) {
      Column({ space: OrbitTokens.SPACE_XS }) {
        Row() {
          Text('ATTACH @ CONTEXT')
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Bold))
          Blank()
          Stack({ alignContent: Alignment.Center }) {
            Text('×')
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Bold))
          }
          .width(OrbitTokens.HIT_MIN)
          .height(OrbitTokens.HIT_MIN)
          .accessibilityText('Dismiss context mention popover')
          .onClick(() => {
            const trig = this.parseTrailingMention();
            if (trig !== null) {
              this.dismissedTriggerToken = trig.rawToken;
            }
          })
        }
        .width('100%')
        .padding({ left: OrbitTokens.SPACE_SM })
        .alignItems(VerticalAlign.Center)

        ForEach(
          this.mentionCandidates(),
          (item: ComposerMentionCandidate) => {
            Row({ space: OrbitTokens.SPACE_SM }) {
              Text(item.tokenLabel)
                .attributeModifier(
                  orbitTextMono(
                    item.kind === 'file'
                      ? this.store.palette.fgPrimary
                      : this.store.palette.accentPrimary,
                    FontWeight.Medium
                  )
                )
                .maxLines(1)
                .textOverflow({ overflow: TextOverflow.Ellipsis })
                .constraintSize({ maxWidth: '52%' })

              Text(item.kind === 'file' ? item.subtitle : item.title)
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                .maxLines(1)
                .textOverflow({ overflow: TextOverflow.Ellipsis })
                .layoutWeight(1)

              if (item.gitStatus.length > 0) {
                Text(item.gitStatus)
                  .attributeModifier(
                    orbitTextMono(this.store.palette.statusWarning, FontWeight.Bold)
                  )
              }
            }
            .width('100%')
            .height(36)
            .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
            .borderRadius(OrbitTokens.PILL_RADIUS)
            .alignItems(VerticalAlign.Center)
            .onClick(() => {
              const trig = this.parseTrailingMention();
              this.store.attachInlineMention(item, trig !== null ? trig.strippedText : '');
              this.dismissedTriggerToken = '';
            })
          },
          (item: ComposerMentionCandidate) => item.key
        )
      }
      .width('100%')
      .padding(OrbitTokens.SPACE_SM)
      .backgroundColor(this.store.palette.elevated)
      .borderRadius(OrbitTokens.CARD_RADIUS)
      .border({
        width: OrbitTokens.HAIRLINE,
        color: this.store.palette.hairline
      })
      .shadow({
        radius: 12,
        color: '#22000000',
        offsetX: 0,
        offsetY: -2
      })
    }
  }

  @Builder
  private renderQueuedPromptsTray() {
    if (this.store.activeQueuedPrompts.length > 0) {
      Column({ space: OrbitTokens.SPACE_XS }) {
        ForEach(
          this.store.activeQueuedPrompts,
          (qItem: QueuedPromptItem, idx: number) => {
            Row({ space: OrbitTokens.SPACE_XS }) {
              Text(`#${idx + 1}`)
                .attributeModifier(orbitTextMono(this.store.palette.accentPrimary, FontWeight.Bold))

              Text(qItem.displayPrompt)
                .attributeModifier(orbitTextMeta(this.store.palette.fgPrimary, FontWeight.Normal))
                .maxLines(1)
                .textOverflow({ overflow: TextOverflow.Ellipsis })
                .layoutWeight(1)

              Row() {
                Text('Edit')
                  .attributeModifier(
                    orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium)
                  )
              }
              .height(28)
              .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
              .backgroundColor(this.store.palette.subtleSurface)
              .borderRadius(OrbitTokens.PILL_RADIUS)
              .alignItems(VerticalAlign.Center)
              .onClick(() => {
                this.store.editQueuedPrompt(qItem.id);
              })

              Row() {
                Text('Send Now')
                  .attributeModifier(
                    orbitTextMeta(this.store.palette.accentPrimary, FontWeight.Medium)
                  )
              }
              .height(28)
              .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
              .backgroundColor(this.store.palette.accentPrimarySoft)
              .borderRadius(OrbitTokens.PILL_RADIUS)
              .alignItems(VerticalAlign.Center)
              .onClick(() => {
                this.store.sendQueuedPromptNow(qItem.id);
              })

              Stack({ alignContent: Alignment.Center }) {
                Text('×')
                  .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Bold))
              }
              .width(OrbitTokens.HIT_MIN)
              .height(OrbitTokens.HIT_MIN)
              .accessibilityText('Remove queued prompt')
              .onClick(() => {
                this.store.removeQueuedPrompt(qItem.id);
              })
            }
            .width('100%')
            .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
            .padding({
              left: OrbitTokens.SPACE_MD,
              right: 0
            })
            .backgroundColor(this.store.palette.elevated)
            .borderRadius(OrbitTokens.CARD_RADIUS)
            .border({
              width: OrbitTokens.HAIRLINE,
              color: this.store.palette.hairline
            })
            .alignItems(VerticalAlign.Center)
          },
          (qItem: QueuedPromptItem) => qItem.id
        )
      }
      .width('100%')
    }
  }

  build() {
    Column({ space: OrbitTokens.SPACE_SM }) {
      this.renderInlineSlashPopover()
      this.renderInlineMentionPopover()
      this.renderQueuedPromptsTray()

      Column() {
        this.renderContextChipsTray()

        TextArea({
          placeholder: 'Ask anything, @ for files & diff, / for commands…',
          text: this.store.composerText
        })
          .attributeModifier(
            orbitComposerArea(this.store.palette.fgPrimary, this.store.palette.fgMuted)
          )
          .width('100%')
          .constraintSize({ minHeight: OrbitTokens.HIT_MIN, maxHeight: 132 })
          .padding({
            left: OrbitTokens.SPACE_MD,
            right: OrbitTokens.SPACE_MD,
            top: OrbitTokens.SPACE_SM,
            bottom: OrbitTokens.SPACE_XS
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

        Row({ space: OrbitTokens.SPACE_XS }) {
          Stack({ alignContent: Alignment.Center }) {
            Row() {
              Text('+')
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold))
            }
            .width(28)
            .height(28)
            .justifyContent(FlexAlign.Center)
            .backgroundColor(this.store.palette.subtleSurface)
            .borderRadius(14)
          }
          .width(OrbitTokens.HIT_MIN)
          .height(OrbitTokens.HIT_MIN)
          .accessibilityText('Attach files, diff, or slash command')
          .onClick(() => {
            this.store.openContextPicker();
          })

          Row({ space: OrbitTokens.SPACE_XS }) {
            Row()
              .width(OrbitTokens.DOT_SIZE)
              .height(OrbitTokens.DOT_SIZE)
              .borderRadius(3)
              .backgroundColor(this.providerDotColor())
            Text(this.store.currentModelPillLabel())
              .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium))
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
              .flexShrink(1)
          }
          .height(OrbitTokens.HIT_MIN)
          .padding({ left: OrbitTokens.SPACE_XS, right: OrbitTokens.SPACE_XS })
          .alignItems(VerticalAlign.Center)
          .flexShrink(1)
          .onClick(() => {
            this.store.showModelPickerSheet = true;
            this.store.triggerHaptic(8);
          })

          Row({ space: OrbitTokens.SPACE_XS }) {
            Row()
              .width(OrbitTokens.DOT_SIZE)
              .height(OrbitTokens.DOT_SIZE)
              .borderRadius(3)
              .backgroundColor(this.modeToneColor())
            Text(this.store.currentModePillLabel())
              .attributeModifier(orbitTextMeta(this.modeToneColor(), FontWeight.Medium))
              .maxLines(1)
          }
          .height(OrbitTokens.HIT_MIN)
          .padding({ left: OrbitTokens.SPACE_XS, right: OrbitTokens.SPACE_XS })
          .alignItems(VerticalAlign.Center)
          .onClick(() => {
            this.store.cycleSessionMode();
          })

          Blank()

          if (this.isAgentRunning() && this.canSend()) {
            Stack({ alignContent: Alignment.Center }) {
              Row({ space: OrbitTokens.SPACE_XS }) {
                Text('■')
                  .attributeModifier(orbitTextMeta(this.store.palette.statusDanger, FontWeight.Bold))
                Text('Stop')
                  .attributeModifier(
                    orbitTextMeta(this.store.palette.statusDanger, FontWeight.Medium)
                  )
              }
              .height(32)
              .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
              .backgroundColor(this.store.palette.statusDangerSoft)
              .borderRadius(16)
              .alignItems(VerticalAlign.Center)
            }
            .height(OrbitTokens.HIT_MIN)
            .constraintSize({ minWidth: OrbitTokens.HIT_MIN })
            .accessibilityText('Stop active agent')
            .onClick(() => {
              this.store.interruptCurrentAgent();
            })

            Stack({ alignContent: Alignment.Center }) {
              Row() {
                Text('Queue')
                  .attributeModifier(orbitTextMeta(this.store.palette.onAccent, FontWeight.Bold))
              }
              .height(32)
              .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
              .backgroundColor(this.store.palette.accentPrimaryFill)
              .borderRadius(16)
              .alignItems(VerticalAlign.Center)
            }
            .height(OrbitTokens.HIT_MIN)
            .constraintSize({ minWidth: OrbitTokens.HIT_MIN })
            .margin({ right: OrbitTokens.SPACE_XS })
            .accessibilityText('Queue next prompt')
            .onClick(() => {
              this.store.enqueueCurrentDraft();
            })
          } else {
            Stack({ alignContent: Alignment.Center }) {
              Row() {
                Text(this.isAgentRunning() ? '■' : '↑')
                  .attributeModifier(
                    orbitTextSubhead(
                      this.isAgentRunning() || this.canSend()
                        ? this.store.palette.onAccent
                        : this.store.palette.fgMuted,
                      FontWeight.Bold
                    )
                  )
              }
              .width(34)
              .height(34)
              .justifyContent(FlexAlign.Center)
              .borderRadius(17)
              .backgroundColor(
                this.isAgentRunning()
                  ? this.store.palette.statusDanger
                  : this.canSend()
                    ? this.store.palette.accentPrimaryFill
                    : this.store.palette.subtleSurface
              )
            }
            .width(OrbitTokens.HIT_MIN)
            .height(OrbitTokens.HIT_MIN)
            .accessibilityText(this.isAgentRunning() ? 'Stop active agent' : 'Send prompt')
            .onClick(() => {
              if (this.isAgentRunning()) {
                this.store.interruptCurrentAgent();
              } else {
                this.store.submitComposerPrompt();
              }
            })
          }
        }
        .width('100%')
        .padding({ left: OrbitTokens.SPACE_XS, right: OrbitTokens.SPACE_XS, bottom: 2 })
        .alignItems(VerticalAlign.Center)
      }
      .width('100%')
      .backgroundColor(this.store.palette.elevated)
      .borderRadius(OrbitTokens.COMPOSER_RADIUS)
      .border({
        width: OrbitTokens.HAIRLINE,
        color: this.store.palette.hairline
      })
    }
    .width('100%')
    .padding({
      left: OrbitTokens.SPACE_LG,
      right: OrbitTokens.SPACE_LG,
      top: OrbitTokens.SPACE_SM,
      bottom: Math.max(
        OrbitTokens.SPACE_SM,
        this.store.keyboardHeightVp > 0 ? this.store.keyboardHeightVp : this.store.safeBottomVp
      )
    })
  }
}
