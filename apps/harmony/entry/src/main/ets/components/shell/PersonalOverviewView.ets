import {
  BriefingDecisionDto,
  HostCardDto,
  SidebarConversationDto,
  basenameOfPath,
  formatCompactTopBarConnectionLabel
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { OrbitTheme } from '../../theme/OrbitTheme';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitButtonSubhead,
  orbitComposerArea,
  orbitTextBody,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead,
  orbitTextTitle
} from '../../theme/OrbitTokens';
import {
  dismissHarmonyKeyboard,
  resolveHostStatusColor
} from '../common/OrbitCommonControls';

@ComponentV2
export struct PersonalOverviewView {
  @Local overviewPromptDraft: string = '';
  @Local isRefreshing: boolean = false;

  private refreshTimerId: number = -1;
  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  aboutToAppear(): void {
    this.store.refreshPersonalAgentBriefing(true);
  }

  aboutToDisappear(): void {
    if (this.refreshTimerId !== -1) {
      clearTimeout(this.refreshTimerId);
      this.refreshTimerId = -1;
    }
  }

  @Computed
  get onlineHostCount(): number {
    let count = 0;
    for (let i = 0; i < this.store.hosts.length; i++) {
      const st = this.store.hosts[i].connection_state;
      if (st === 'online_direct' || st === 'online_relay') {
        count += 1;
      }
    }
    return count;
  }

  @Computed
  get activeSessionList(): SidebarConversationDto[] {
    const out: SidebarConversationDto[] = [];
    const sv = this.store.sidebarView;
    if (sv === null) {
      return out;
    }
    const seen: Set<string> = new Set<string>();
    const addIfRunning = (list: SidebarConversationDto[]): void => {
      for (let i = 0; i < list.length; i++) {
        const c = list[i];
        const key = `${c.host_id}::${c.agent.agent_id}`;
        if ((c.agent.is_running || c.agent.is_awaiting_permission) && !seen.has(key)) {
          seen.add(key);
          out.push(c);
        }
      }
    };
    addIfRunning(sv.needs_attention);
    addIfRunning(sv.pinned_conversations);
    for (let i = 0; i < sv.projects.length; i++) {
      addIfRunning(sv.projects[i].conversations);
    }
    return out;
  }

  @Computed
  get batchEligibleApprovalCount(): number {
    let count = 0;
    const list = this.store.briefingNeedsYou;
    for (let i = 0; i < list.length; i++) {
      if (list[i].batch_eligible && !list[i].human_only && list[i].request_id !== null) {
        count += 1;
      }
    }
    return count;
  }

  @Computed
  get calmOneLineStatus(): string {
    const total = this.store.hosts.length;
    const online = this.onlineHostCount;
    const working = this.activeSessionList.length;
    const pending = this.store.totalPendingApprovals;
    if (pending > 0) {
      return `${online}/${total} online · ${pending} waiting for your decision`;
    }
    if (working > 0) {
      return `${online}/${total} online · ${working} session${working === 1 ? '' : 's'} active`;
    }
    if (online === 0 && total > 0) {
      return `All ${total} host${total === 1 ? '' : 's'} offline · Tap a host to reconnect`;
    }
    return `${online}/${total} host${total === 1 ? '' : 's'} online · Quiet & ready`;
  }

  private hasDraftText(): boolean {
    return this.overviewPromptDraft.trim().length > 0;
  }

  private formatVoiceTimer(sec: number): string {
    const m = Math.floor(sec / 60);
    const s = sec % 60;
    return `${m}:${s.toString().padStart(2, '0')}`;
  }

  private handleRefresh(): void {
    this.isRefreshing = true;
    this.store.reloadFleetAsync();
    this.store.refreshPersonalAgentBriefing(true);
    if (this.refreshTimerId !== -1) {
      clearTimeout(this.refreshTimerId);
    }
    this.refreshTimerId = setTimeout(() => {
      this.isRefreshing = false;
      this.refreshTimerId = -1;
    }, 450);
  }

  private isHostOnline(host: HostCardDto): boolean {
    return host.connection_state === 'online_direct' || host.connection_state === 'online_relay';
  }

  private hostRouteMeta(host: HostCardDto): string {
    const label = formatCompactTopBarConnectionLabel(host.connection_state, host.via_ssh_tunnel);
    if (host.last_ping_ms !== null && host.last_ping_ms !== undefined) {
      return `${label} · ${host.last_ping_ms}ms`;
    }
    return label;
  }

  private submitOverviewDraft(): void {
    const text = this.overviewPromptDraft.trim();
    if (text.length === 0) {
      return;
    }
    dismissHarmonyKeyboard(this.getUIContext());
    this.overviewPromptDraft = '';
    this.store.submitPersonalAgentPrompt(text);
  }

  @Builder
  private renderNeedsDecisionSection() {
    if (this.store.briefingNeedsYou.length > 0) {
      Column({ space: OrbitTokens.SPACE_SM }) {
        Row({ space: OrbitTokens.SPACE_SM }) {
          Row()
            .width(OrbitTokens.DOT_SIZE)
            .height(OrbitTokens.DOT_SIZE)
            .borderRadius(3)
            .backgroundColor(this.store.palette.statusWarning)

          Text($r('app.string.overview_needs_decision'))
            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
            .layoutWeight(1)

          if (this.batchEligibleApprovalCount > 0) {
            Button({ type: ButtonType.Capsule, stateEffect: true }) {
              Text($r('app.string.overview_batch_approve_safe'))
                .attributeModifier(orbitTextMeta(this.store.palette.onAccent, FontWeight.Medium))
            }
            .height(32)
            .padding({ left: 12, right: 12 })
            .backgroundColor(this.store.palette.accentPrimaryFill)
            .onClick(() => {
              this.store.approveSafeBatchPermissions();
            })
          }
        }
        .width('100%')
        .alignItems(VerticalAlign.Center)

        ForEach(
          this.store.briefingNeedsYou,
          (item: BriefingDecisionDto) => {
            Column({ space: OrbitTokens.SPACE_SM }) {
              Row({ space: OrbitTokens.SPACE_SM }) {
                Column({ space: 2 }) {
                  Text(item.agent_title.length > 0 ? item.agent_title : item.kind)
                    .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                    .maxLines(1)
                    .textOverflow({ overflow: TextOverflow.Ellipsis })
                  Text(`${item.host_name} · ${item.kind}`)
                    .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                    .maxLines(1)
                    .textOverflow({ overflow: TextOverflow.Ellipsis })
                }
                .alignItems(HorizontalAlign.Start)
                .layoutWeight(1)
                .onClick(() => {
                  this.store.openSessionFromDeepLink(item.host_id, item.agent_id, 'permission_required');
                })

                if (item.batch_eligible && !item.human_only) {
                  Text('Low risk')
                    .attributeModifier(orbitTextMeta(this.store.palette.statusDirect, FontWeight.Medium))
                    .padding({ left: 7, right: 7, top: 2, bottom: 2 })
                    .borderRadius(OrbitTokens.RADIUS_XS)
                    .backgroundColor(this.store.palette.statusDirectSoft)
                }
              }
              .width('100%')
              .alignItems(VerticalAlign.Center)

              Text(item.summary)
                .attributeModifier(orbitTextMono(this.store.palette.fgSecondary))
                .maxLines(2)
                .textOverflow({ overflow: TextOverflow.Ellipsis })
                .width('100%')
                .padding(OrbitTokens.SPACE_SM)
                .borderRadius(OrbitTokens.RADIUS_SM)
                .attributeModifier(OrbitTheme.recessedWell(this.store.palette))

              if (item.request_id !== null && item.request_id.length > 0 && item.kind === 'permission') {
                Row({ space: OrbitTokens.SPACE_SM }) {
                  Button($r('app.string.action_allow_once'), { type: ButtonType.Capsule, stateEffect: true })
                    .attributeModifier(
                      orbitButtonMeta(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
                    )
                    .height(36)
                    .layoutWeight(1)
                    .onClick(() => {
                      this.store.resolveBriefingDecision(item, 'allow');
                    })

                  Button($r('app.string.action_decline'), { type: ButtonType.Capsule, stateEffect: true })
                    .attributeModifier(
                      orbitButtonMeta(this.store.palette.statusDanger, this.store.palette.statusDangerSoft)
                    )
                    .height(36)
                    .layoutWeight(1)
                    .onClick(() => {
                      this.store.resolveBriefingDecision(item, 'deny');
                    })
                }
                .width('100%')
              }
            }
            .width('100%')
            .padding(OrbitTokens.SPACE_MD)
            .borderRadius(OrbitTokens.RADIUS_LG)
            .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
            .alignItems(HorizontalAlign.Start)
          },
          (item: BriefingDecisionDto): string => item.key
        )
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
    }
  }

  @Builder
  private renderLiveFleetSection() {
    Column({ space: OrbitTokens.SPACE_SM }) {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Text($r('app.string.overview_live_fleet'))
          .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Medium))
          .layoutWeight(1)

        Button({ type: ButtonType.Normal, stateEffect: true }) {
          Row({ space: 4 }) {
            SymbolGlyph($r('sys.symbol.magnifyingglass'))
              .fontSize(13)
              .fontColor([this.store.palette.iconSecondary])
            Text($r('app.string.nav_search_fleet'))
              .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium))
          }
        }
        .height(32)
        .padding({ left: 8, right: 8 })
        .backgroundColor(Color.Transparent)
        .onClick(() => {
          this.store.openGlobalSearch();
        })

        Button({ type: ButtonType.Normal, stateEffect: true }) {
          Row({ space: 4 }) {
            SymbolGlyph($r('sys.symbol.link'))
              .fontSize(13)
              .fontColor([this.store.palette.iconSecondary])
            Text($r('app.string.overview_open_router_settings'))
              .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium))
          }
        }
        .height(32)
        .padding({ left: 8, right: 4 })
        .backgroundColor(Color.Transparent)
        .onClick(() => {
          this.store.openRouterMeshSheet();
        })
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)

      ForEach(
        this.store.hosts,
        (host: HostCardDto) => {
          Row({ space: OrbitTokens.SPACE_MD }) {
            Row()
              .width(8)
              .height(8)
              .borderRadius(4)
              .backgroundColor(resolveHostStatusColor(host, this.store))

            Column({ space: 2 }) {
              Row({ space: 6 }) {
                Text(host.display_name)
                  .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                  .maxLines(1)
                  .textOverflow({ overflow: TextOverflow.Ellipsis })
                  .flexShrink(1)

                if (host.host_id === this.store.primaryRouterHostId) {
                  Text('Router')
                    .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium))
                    .padding({ left: 6, right: 6, top: 1, bottom: 1 })
                    .borderRadius(OrbitTokens.RADIUS_XS)
                    .backgroundColor(this.store.palette.subtleSurface)
                }
              }
              .alignItems(VerticalAlign.Center)

              Text(`${this.hostRouteMeta(host)} · ${host.agent_count} agents`)
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                .maxLines(1)
                .textOverflow({ overflow: TextOverflow.Ellipsis })
            }
            .alignItems(HorizontalAlign.Start)
            .layoutWeight(1)

            if (host.pending_approval_count > 0) {
              Text(`${host.pending_approval_count}`)
                .attributeModifier(orbitTextMono(this.store.palette.statusWarning, FontWeight.Bold))
                .padding({ left: 8, right: 8, top: 3, bottom: 3 })
                .borderRadius(OrbitTokens.RADIUS_MD)
                .backgroundColor(this.store.palette.statusWarningSoft)
            } else if (!this.isHostOnline(host)) {
              Button($r('app.string.action_reconnect'), { type: ButtonType.Capsule, stateEffect: true })
                .attributeModifier(
                  orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface)
                )
                .height(32)
                .padding({ left: 10, right: 10 })
                .onClick(() => {
                  this.store.reconnectHost(host.host_id);
                })
            } else {
              SymbolGlyph($r('sys.symbol.chevron_right'))
                .fontSize(13)
                .fontColor([this.store.palette.fgMuted])
            }
          }
          .width('100%')
          .constraintSize({ minHeight: 58 })
          .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_MD, top: 10, bottom: 10 })
          .borderRadius(OrbitTokens.RADIUS_LG)
          .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
          .alignItems(VerticalAlign.Center)
          .onClick(() => {
            this.store.selectHost(host.host_id);
            this.store.startNewSessionDraft();
          })
        },
        (host: HostCardDto): string => `ov_host_${host.host_id}_${host.connection_state}_${host.pending_approval_count}`
      )
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  @Builder
  private renderActiveSessionsSection() {
    if (this.activeSessionList.length > 0) {
      Column({ space: OrbitTokens.SPACE_SM }) {
        Text($r('app.string.overview_active_sessions'))
          .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Medium))

        ForEach(
          this.activeSessionList,
          (conv: SidebarConversationDto) => {
            Row({ space: OrbitTokens.SPACE_SM }) {
              if (conv.agent.is_awaiting_permission) {
                SymbolGlyph($r('sys.symbol.exclamationmark_triangle_fill'))
                  .fontSize(14)
                  .fontColor([this.store.palette.statusWarning])
              } else {
                LoadingProgress()
                  .width(15)
                  .height(15)
                  .color(this.store.palette.iconSecondary)
              }

              Column({ space: 2 }) {
                Text(conv.agent.title.trim().length > 0 ? conv.agent.title : conv.agent.provider_label)
                  .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                  .maxLines(1)
                  .textOverflow({ overflow: TextOverflow.Ellipsis })
                Text(`${basenameOfPath(conv.agent.cwd)} · ${conv.agent.status_label}`)
                  .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                  .maxLines(1)
                  .textOverflow({ overflow: TextOverflow.Ellipsis })
              }
              .alignItems(HorizontalAlign.Start)
              .layoutWeight(1)

              SymbolGlyph($r('sys.symbol.chevron_right'))
                .fontSize(13)
                .fontColor([this.store.palette.fgMuted])
            }
            .width('100%')
            .constraintSize({ minHeight: 54 })
            .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD, top: 8, bottom: 8 })
            .borderRadius(OrbitTokens.RADIUS_LG)
            .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
            .alignItems(VerticalAlign.Center)
            .onClick(() => {
              this.store.openSessionFromDeepLink(conv.host_id, conv.agent.agent_id, '');
            })
          },
          (conv: SidebarConversationDto): string => `ov_sess_${conv.host_id}_${conv.agent.agent_id}_${conv.agent.latest_seq}`
        )
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
    }
  }

  @Builder
  private renderDockedPersonalComposer() {
    Column({ space: OrbitTokens.SPACE_XS }) {
      // Inline minimal voice strip ONLY while voice dictation is active
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

          Text(this.formatVoiceTimer(this.store.voiceElapsedSeconds))
            .attributeModifier(orbitTextMono(this.store.palette.fgMuted))

          Button({ type: ButtonType.Circle, stateEffect: true }) {
            SymbolGlyph($r('sys.symbol.xmark'))
              .fontSize(13)
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
        .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_SM, top: 8, bottom: 4 })
        .alignItems(VerticalAlign.Center)
      }

      Row({ space: OrbitTokens.SPACE_SM }) {
        TextArea({
          placeholder: $r('app.string.overview_router_placeholder'),
          text: this.overviewPromptDraft
        })
          .attributeModifier(
            orbitComposerArea(this.store.palette.fgPrimary, this.store.palette.fgMuted)
          )
          .layoutWeight(1)
          .constraintSize({ minHeight: 40, maxHeight: 108 })
          .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_XS, top: 10, bottom: 10 })
          .onChange((val: string) => {
            this.overviewPromptDraft = val;
          })

        // SINGLE stationary right action circle (Codex-style: empty+idle -> Mic, text -> Send)
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
            const beforeLen = this.store.composerText.length;
            this.store.finishVoiceDictation();
            if (this.store.composerText.length > beforeLen) {
              const spoken = this.store.composerText.substring(beforeLen).trim();
              this.store.composerText = this.store.composerText.substring(0, beforeLen);
              if (spoken.length > 0) {
                this.overviewPromptDraft = this.overviewPromptDraft.trim().length > 0
                  ? `${this.overviewPromptDraft.trimEnd()} ${spoken}`
                  : spoken;
              }
            }
          })
        } else if (this.hasDraftText()) {
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
            this.submitOverviewDraft();
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
      .padding({ left: 4, right: 8, top: 4, bottom: 4 })
      .alignItems(VerticalAlign.Bottom)
    }
    .width('100%')
    .constraintSize({ maxWidth: OrbitTokens.STAGE_MAX_READING_WIDTH })
    .borderRadius(OrbitTokens.RADIUS_XXL)
    .attributeModifier(OrbitTheme.dockedCard(this.store.palette))
  }

  build() {
    Column({ space: 0 }) {
      Refresh({ refreshing: $$this.isRefreshing }) {
        Scroll() {
          Column({ space: OrbitTokens.SPACE_XL }) {
            // Humanized calm 1-line status hero
            Column({ space: 6 }) {
              Text($r('app.string.overview_title'))
                .attributeModifier(orbitTextTitle(this.store.palette.fgPrimary, FontWeight.Bold))
              Text(this.calmOneLineStatus)
                .attributeModifier(orbitTextBody(this.store.palette.fgSecondary))
                .maxLines(1)
                .textOverflow({ overflow: TextOverflow.Ellipsis })
            }
            .width('100%')
            .alignItems(HorizontalAlign.Start)
            .padding({ top: OrbitTokens.SPACE_SM })

            this.renderNeedsDecisionSection()
            this.renderActiveSessionsSection()
            this.renderLiveFleetSection()
          }
          .width('100%')
          .constraintSize({ maxWidth: OrbitTokens.STAGE_MAX_READING_WIDTH })
          .padding({
            left: OrbitTokens.SPACE_LG,
            right: OrbitTokens.SPACE_LG,
            top: OrbitTokens.SPACE_SM,
            bottom: OrbitTokens.SPACE_XL
          })
          .alignItems(HorizontalAlign.Start)
        }
        .width('100%')
        .height('100%')
        .scrollBar(BarState.Off)
        .edgeEffect(EdgeEffect.Spring)
        .onScrollStart(() => {
          dismissHarmonyKeyboard(this.getUIContext());
        })
      }
      .onRefreshing(() => {
        this.handleRefresh();
      })
      .width('100%')
      .layoutWeight(1)

      Column() {
        this.renderDockedPersonalComposer()
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
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.canvas)
  }
}
