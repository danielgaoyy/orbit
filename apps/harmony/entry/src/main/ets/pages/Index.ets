import {
  AgentCardDto,
  DiffFileDto,
  DiffLineDto,
  HighlightedLineDto,
  HighlightedTokenDto,
  HostCardDto,
  InlineSpanDto,
  OrbitHarmonyStore,
  QuestionItemDto,
  RichBlockDto,
  TerminalRowDto,
  TerminalSpanDto,
  TimelineItemDto
} from '../model/OrbitHarmonyStore';

@Entry
@ComponentV2
struct Index {
  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();
  @Local activeTab: number = 0; // 0: Fleet, 1: Chat, 2: Git Diff, 3: PTY Terminal
  @Local pairingUriInput: string = '';
  @Local composerInput: string = '';
  @Local newAgentCwd: string = '~';
  @Local newAgentProvider: string = 'antigravity';
  @Local terminalCommandInput: string = '';

  private currentWorkspaceCwd(): string {
    const detail = this.store.currentHostDetail;
    if (detail !== null && detail.workspaces.length > 0) {
      return detail.workspaces[0].cwd;
    }
    if (detail !== null && detail.agents.length > 0) {
      return detail.agents[0].cwd;
    }
    return '~';
  }

  @Builder
  renderRichBlocks(blocks: RichBlockDto[]) {
    Column({ space: 6 }) {
      ForEach(blocks, (blk: RichBlockDto, bIdx: number) => {
        if (blk.kind === 'code_block' && blk.lines !== undefined) {
          Column({ space: 2 }) {
            Text(blk.language ?? 'code')
              .fontSize(11)
              .fontColor('#94A3B8')
              .fontFamily('monospace')
            Scroll() {
              Column({ space: 1 }) {
                ForEach(blk.lines, (ln: HighlightedLineDto) => {
                  Text() {
                    Span(`${ln.line_number.toString().padStart(3, ' ')}  `)
                      .fontColor('#475569')
                    ForEach(ln.tokens, (tok: HighlightedTokenDto) => {
                      Span(tok.text)
                        .fontColor(tok.fg_hex)
                        .fontWeight(tok.bold ? FontWeight.Bold : FontWeight.Normal)
                    })
                  }
                  .fontFamily('monospace')
                  .fontSize(12)
                }, (ln: HighlightedLineDto) => ln.line_number.toString())
              }
              .alignItems(HorizontalAlign.Start)
            }
            .scrollable(ScrollDirection.Horizontal)
          }
          .width('100%')
          .padding(10)
          .backgroundColor('#070A12')
          .borderRadius(8)
        } else if (blk.spans !== undefined) {
          Text() {
            ForEach(blk.spans, (sp: InlineSpanDto) => {
              Span(sp.text)
                .fontWeight(sp.bold ? FontWeight.Bold : FontWeight.Normal)
                .fontStyle(sp.italic ? FontStyle.Italic : FontStyle.Normal)
                .fontColor(sp.inline_code ? '#60A5FA' : '#F1F5F9')
                .fontFamily(sp.inline_code ? 'monospace' : 'sans-serif')
            })
          }
          .fontSize(blk.kind === 'heading' ? 16 : 14)
          .lineHeight(21)
        }
      }, (_blk: RichBlockDto, idx: number) => idx.toString())
    }
    .alignItems(HorizontalAlign.Start)
    .width('100%')
  }

  @Builder
  renderFleetTab() {
    Column({ space: 12 }) {
      Row({ space: 8 }) {
        TextInput({ placeholder: 'Paste orbit://pair/... URI', text: this.pairingUriInput })
          .layoutWeight(1)
          .backgroundColor('#131A29')
          .fontColor('#F8FAFC')
          .placeholderColor('#64748B')
          .onChange((v: string) => {
            this.pairingUriInput = v;
          })
        Button('Pair')
          .backgroundColor('#3B82F6')
          .onClick(() => {
            this.store.pairHost(this.pairingUriInput);
            this.pairingUriInput = '';
          })
      }
      .width('100%')

      if (this.store.lastError.length > 0) {
        Text(this.store.lastError)
          .fontSize(12)
          .fontColor('#F87171')
      }

      List({ space: 10 }) {
        Repeat<HostCardDto>(this.store.hosts)
          .each((ri: RepeatItem<HostCardDto>) => {
            ListItem() {
              Column({ space: 6 }) {
                Row() {
                  Text(ri.item.display_name)
                    .fontSize(16)
                    .fontWeight(FontWeight.Bold)
                    .fontColor('#F8FAFC')
                  Blank()
                  Text(ri.item.connection_state)
                    .fontSize(11)
                    .fontColor(
                      ri.item.connection_state === 'online_direct'
                        ? '#4ADE80'
                        : ri.item.connection_state === 'online_relay'
                          ? '#C084FC'
                          : '#94A3B8'
                    )
                }
                .width('100%')

                Text(`${ri.item.agent_count} agents · ${ri.item.pending_approval_count} awaiting approval`)
                  .fontSize(12)
                  .fontColor('#94A3B8')
              }
              .width('100%')
              .padding(14)
              .backgroundColor(
                this.store.selectedHostId === ri.item.host_id ? '#1E293B' : '#111827'
              )
              .borderRadius(12)
              .onClick(() => {
                this.store.selectHost(ri.item.host_id);
              })
            }
          })
          .key((h: HostCardDto) => h.host_id)
          .virtualScroll()
      }
      .width('100%')
      .height(210)

      if (this.store.currentHostDetail !== null) {
        Row({ space: 8 }) {
          Text('Agent Sessions')
            .fontSize(14)
            .fontWeight(FontWeight.Medium)
            .fontColor('#CBD5E1')
          Blank()
          ForEach(['antigravity', 'codex', 'claude_code'], (prov: string) => {
            Button(prov === 'antigravity' ? 'AGY' : prov === 'codex' ? 'Codex' : 'Claude')
              .height(26)
              .fontSize(11)
              .backgroundColor(this.newAgentProvider === prov ? '#3B82F6' : '#1E293B')
              .onClick(() => {
                this.newAgentProvider = prov;
              })
          }, (p: string) => p)
        }
        .width('100%')

        List({ space: 8 }) {
          Repeat<AgentCardDto>(this.store.currentHostDetail.agents)
            .each((ri: RepeatItem<AgentCardDto>) => {
              ListItem() {
                Row() {
                  Column({ space: 3 }) {
                    Text(ri.item.title)
                      .fontSize(14)
                      .fontWeight(FontWeight.Medium)
                      .fontColor('#F1F5F9')
                    Text(`${ri.item.provider_label} · ${ri.item.cwd}`)
                      .fontSize(11)
                      .fontColor('#64748B')
                  }
                  .alignItems(HorizontalAlign.Start)
                  .layoutWeight(1)

                  Text(ri.item.status_label)
                    .fontSize(11)
                    .fontColor(ri.item.is_awaiting_permission ? '#FBBF24' : '#60A5FA')
                }
                .width('100%')
                .padding(12)
                .backgroundColor('#111827')
                .borderRadius(10)
                .onClick(() => {
                  this.store.selectAgent(ri.item.agent_id);
                  this.activeTab = 1;
                })
              }
            })
            .key((a: AgentCardDto) => a.agent_id)
            .virtualScroll()
        }
        .layoutWeight(1)
        .width('100%')
      }
    }
    .padding(14)
    .width('100%')
    .layoutWeight(1)
  }

  @Builder
  renderChatTab() {
    Column({ space: 8 }) {
      List({ space: 10 }) {
        Repeat<TimelineItemDto>(this.store.timelineItems)
          .each((ri: RepeatItem<TimelineItemDto>) => {
            ListItem() {
              Column({ space: 8 }) {
                if (ri.item.item_type === 'permission_request') {
                  Text(`Permission Required: ${ri.item.title ?? ''}`)
                    .fontSize(13)
                    .fontWeight(FontWeight.Bold)
                    .fontColor('#FBBF24')
                  if (ri.item.resolved_decision !== null && ri.item.resolved_decision !== undefined) {
                    Text(`Resolved: ${ri.item.resolved_decision}`)
                      .fontSize(12)
                      .fontColor('#4ADE80')
                  } else {
                    Row({ space: 8 }) {
                      ForEach(ri.item.allowed_decisions ?? ['allow', 'deny'], (dec: string) => {
                        Button(dec)
                          .height(30)
                          .fontSize(12)
                          .backgroundColor(dec.includes('deny') ? '#DC2626' : '#2563EB')
                          .onClick(() => {
                            if (ri.item.request_id !== undefined) {
                              this.store.resolvePermission(ri.item.request_id, dec);
                            }
                          })
                      }, (d: string) => d)
                    }
                  }
                } else if (ri.item.item_type === 'question_request' && ri.item.questions !== undefined) {
                  ForEach(ri.item.questions, (q: QuestionItemDto) => {
                    Column({ space: 6 }) {
                      Text(q.question)
                        .fontSize(13)
                        .fontWeight(FontWeight.Medium)
                        .fontColor('#7DD3FC')
                      Flex({ wrap: FlexWrap.Wrap }) {
                        ForEach(q.options, (opt: string) => {
                          Button(opt)
                            .height(28)
                            .fontSize(11)
                            .margin({ right: 6, bottom: 6 })
                            .backgroundColor('#1E293B')
                            .onClick(() => {
                              if (ri.item.request_id !== undefined) {
                                this.store.answerSingleQuestion(ri.item.request_id, opt);
                              }
                            })
                        }, (o: string) => o)
                      }
                    }
                    .alignItems(HorizontalAlign.Start)
                  }, (q: QuestionItemDto) => q.id)
                } else if (ri.item.blocks !== undefined) {
                  this.renderRichBlocks(ri.item.blocks)
                } else {
                  Text(ri.item.summary_title ?? ri.item.message ?? ri.item.text ?? '')
                    .fontSize(12)
                    .fontColor('#94A3B8')
                    .fontFamily('monospace')
                }
              }
              .width('100%')
              .padding(12)
              .backgroundColor(ri.item.item_type === 'user_prompt' ? '#172554' : '#111827')
              .borderRadius(12)
              .alignItems(HorizontalAlign.Start)
            }
          })
          .key((item: TimelineItemDto) => `${item.item_type}_${item.seq}`)
          .virtualScroll()
      }
      .layoutWeight(1)
      .width('100%')

      Row({ space: 8 }) {
        TextInput({ placeholder: 'Send prompt or create session...', text: this.composerInput })
          .layoutWeight(1)
          .backgroundColor('#131A29')
          .fontColor('#F8FAFC')
          .placeholderColor('#64748B')
          .onChange((v: string) => {
            this.composerInput = v;
          })
        Button('Stop')
          .backgroundColor('#991B1B')
          .onClick(() => {
            this.store.interruptCurrentAgent();
          })
        Button('Send')
          .backgroundColor('#2563EB')
          .onClick(() => {
            if (this.composerInput.trim().length === 0) {
              return;
            }
            if (this.store.selectedAgentId.length > 0) {
              this.store.sendPrompt(this.composerInput.trim());
            } else {
              this.store.createAgent(
                this.newAgentCwd,
                this.newAgentProvider,
                'accept_edits',
                this.composerInput.trim()
              );
            }
            this.composerInput = '';
          })
      }
      .width('100%')
    }
    .padding(12)
    .width('100%')
    .layoutWeight(1)
  }

  @Builder
  renderGitTab() {
    Column({ space: 10 }) {
      List({ space: 12 }) {
        Repeat<DiffFileDto>(this.store.gitDiffFiles)
          .each((ri: RepeatItem<DiffFileDto>) => {
            ListItem() {
              Column({ space: 4 }) {
                Row() {
                  Text(ri.item.path)
                    .fontSize(12)
                    .fontFamily('monospace')
                    .fontColor('#E2E8F0')
                  Blank()
                  Text(`+${ri.item.additions} -${ri.item.deletions}`)
                    .fontSize(11)
                    .fontFamily('monospace')
                    .fontColor('#4ADE80')
                }
                .width('100%')

                ForEach(ri.item.lines, (ln: DiffLineDto, idx: number) => {
                  Text(ln.content)
                    .width('100%')
                    .fontSize(11)
                    .fontFamily('monospace')
                    .fontColor(
                      ln.kind === 'added'
                        ? '#86EFAC'
                        : ln.kind === 'removed'
                          ? '#FCA5A5'
                          : '#CBD5E1'
                    )
                    .backgroundColor(
                      ln.kind === 'added'
                        ? '#052E16'
                        : ln.kind === 'removed'
                          ? '#450A0A'
                          : '#0B0F19'
                    )
                    .padding({ left: 6, right: 6, top: 1, bottom: 1 })
                }, (_l: DiffLineDto, i: number) => i.toString())
              }
              .padding(10)
              .backgroundColor('#0B0F19')
              .borderRadius(10)
            }
          })
          .key((f: DiffFileDto) => f.path)
          .virtualScroll()
      }
      .layoutWeight(1)
      .width('100%')
    }
    .padding(12)
    .width('100%')
    .layoutWeight(1)
  }

  @Builder
  renderTerminalTab() {
    Column({ space: 8 }) {
      List({ space: 1 }) {
        Repeat<TerminalRowDto>(this.store.terminalRows)
          .each((ri: RepeatItem<TerminalRowDto>) => {
            ListItem() {
              Text() {
                ForEach(ri.item.spans, (sp: TerminalSpanDto) => {
                  Span(sp.text)
                    .fontColor(sp.fg_hex)
                    .fontWeight(sp.bold ? FontWeight.Bold : FontWeight.Normal)
                })
              }
              .fontFamily('monospace')
              .fontSize(12)
            }
          })
          .key((r: TerminalRowDto) => r.row_index.toString())
          .virtualScroll()
      }
      .layoutWeight(1)
      .width('100%')
      .padding(8)
      .backgroundColor('#05080F')
      .borderRadius(10)

      Row({ space: 6 }) {
        ForEach(['ESC', 'TAB', 'CTRL+C', 'UP', 'DOWN'], (k: string) => {
          Button(k)
            .height(28)
            .fontSize(11)
            .backgroundColor('#1E293B')
            .onClick(() => {
              const seq =
                k === 'ESC'
                  ? '\x1b'
                  : k === 'TAB'
                    ? '\t'
                    : k === 'CTRL+C'
                      ? '\x03'
                      : k === 'UP'
                        ? '\x1b[A'
                        : '\x1b[B';
              this.store.sendTerminalKeys(seq);
            })
        }, (k: string) => k)
      }
      .width('100%')

      Row({ space: 8 }) {
        TextInput({ placeholder: 'Shell input...', text: this.terminalCommandInput })
          .layoutWeight(1)
          .fontFamily('monospace')
          .backgroundColor('#131A29')
          .fontColor('#F8FAFC')
          .onChange((v: string) => {
            this.terminalCommandInput = v;
          })
        Button('Run')
          .backgroundColor('#2563EB')
          .onClick(() => {
            this.store.sendTerminalKeys(`${this.terminalCommandInput}\r`);
            this.terminalCommandInput = '';
          })
      }
      .width('100%')
    }
    .padding(12)
    .width('100%')
    .layoutWeight(1)
  }

  build() {
    Column() {
      // Top Navigation Bar
      Row({ space: 6 }) {
        Text('Orbit 7')
          .fontSize(18)
          .fontWeight(FontWeight.Bold)
          .fontColor('#F8FAFC')
        Blank()
        Button('Fleet')
          .height(30)
          .fontSize(12)
          .backgroundColor(this.activeTab === 0 ? '#2563EB' : '#1E293B')
          .onClick(() => {
            this.activeTab = 0;
          })
        Button('Stream')
          .height(30)
          .fontSize(12)
          .backgroundColor(this.activeTab === 1 ? '#2563EB' : '#1E293B')
          .onClick(() => {
            this.activeTab = 1;
          })
        Button('Diff')
          .height(30)
          .fontSize(12)
          .backgroundColor(this.activeTab === 2 ? '#2563EB' : '#1E293B')
          .onClick(() => {
            this.activeTab = 2;
            this.store.openGitReview(this.currentWorkspaceCwd());
          })
        Button('PTY')
          .height(30)
          .fontSize(12)
          .backgroundColor(this.activeTab === 3 ? '#2563EB' : '#1E293B')
          .onClick(() => {
            this.activeTab = 3;
            this.store.startPty(this.currentWorkspaceCwd());
          })
      }
      .width('100%')
      .padding({ left: 14, right: 14, top: 12, bottom: 10 })
      .backgroundColor('#0D1321')

      if (this.activeTab === 0) {
        this.renderFleetTab()
      } else if (this.activeTab === 1) {
        this.renderChatTab()
      } else if (this.activeTab === 2) {
        this.renderGitTab()
      } else {
        this.renderTerminalTab()
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor('#090D16')
  }
}
