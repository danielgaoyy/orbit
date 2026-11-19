import {
  HostCardDto,
  ProviderQuotaDto,
  QuotaWindowDto,
  formatConnectionStateLabel,
  formatProviderLabel,
  formatQuotaResetCountdown,
  formatRouteModeLabel
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitButtonSubhead,
  orbitInputMono,
  orbitInputSubhead,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead,
  orbitTextTitle
} from '../../theme/OrbitTokens';

interface ThemeOptionItem {
  id: string;
  label: string;
  subtitle: string;
}

const THEME_ROW_TOP: ThemeOptionItem[] = [
  { id: 'system', label: 'System', subtitle: 'Match system' },
  { id: 'light', label: 'Light', subtitle: 'Daylight paper' }
];

const THEME_ROW_BOTTOM: ThemeOptionItem[] = [
  { id: 'dark', label: 'Graphite', subtitle: 'Quiet graphite' },
  { id: 'oled', label: 'OLED', subtitle: 'True pitch black' }
];

const ROUTE_MODES: string[] = ['auto', 'direct_only', 'ssh_tunnel', 'relay_only'];

@ComponentV2
export struct HostSettingsSheet {
  @Local pairingUriDraft: string = '';
  @Local customNameDraft: string = '';
  @Local routeModeDraft: string = 'auto';
  @Local sshAliasDraft: string = '';
  @Local portDraft: string = '6868';
  @Local directEndpointDraft: string = '';
  @Local relayUrlDraft: string = '';
  @Local hydratedHostId: string = '';

  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  private activeHostCard(): HostCardDto | null {
    const detail = this.store.currentHostDetail;
    if (detail !== null) {
      return detail.host;
    }
    for (let i = 0; i < this.store.hosts.length; i++) {
      if (this.store.hosts[i].host_id === this.store.selectedHostId) {
        return this.store.hosts[i];
      }
    }
    return null;
  }

  private ensureDraftHydrated(): void {
    const h = this.activeHostCard();
    if (h !== null && h.host_id !== this.hydratedHostId) {
      this.hydratedHostId = h.host_id;
      this.customNameDraft = h.custom_name ?? '';
      this.routeModeDraft = h.route_mode;
      this.sshAliasDraft = h.ssh_alias ?? '';
      this.portDraft = h.remote_daemon_port.toString();
      this.directEndpointDraft = h.custom_direct_endpoint ?? '';
      this.relayUrlDraft = h.relay_url ?? '';
    }
  }

  private quotas(): ProviderQuotaDto[] {
    const detail = this.store.currentHostDetail;
    return detail !== null ? detail.quotas : [];
  }

  private hostDotColor(host: HostCardDto): string {
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
    if (host.last_error !== null && host.last_error.length > 0) {
      return this.store.palette.statusDanger;
    }
    return this.store.palette.fgMuted;
  }

  private hostDiagnosticsSubtitle(host: HostCardDto): string {
    const parts: string[] = [formatConnectionStateLabel(host.connection_state, host.via_ssh_tunnel)];
    if (host.last_ping_ms !== null) {
      parts.push(`${host.last_ping_ms}ms`);
    }
    if (host.daemon_version !== null && host.daemon_version.length > 0) {
      parts.push(`v${host.daemon_version}`);
    }
    if (host.os_arch !== null && host.os_arch.length > 0) {
      parts.push(host.os_arch);
    }
    return parts.join(' · ');
  }

  private quotaSemanticColor(usedPercent: number): string {
    if (usedPercent > 90) {
      return this.store.palette.statusDanger;
    }
    if (usedPercent >= 70) {
      return this.store.palette.statusWarning;
    }
    return this.store.palette.statusDirect;
  }

  aboutToAppear(): void {
    this.ensureDraftHydrated();
  }

  @Builder
  private renderThemeCard(opt: ThemeOptionItem) {
    Column({ space: 2 }) {
      Row() {
        Text(opt.label)
          .attributeModifier(
            orbitTextSubhead(
              this.store.themeMode === opt.id
                ? this.store.palette.accentPrimary
                : this.store.palette.fgPrimary,
              FontWeight.Bold
            )
          )
        Blank()
        if (this.store.themeMode === opt.id) {
          Text('✓')
            .attributeModifier(orbitTextSubhead(this.store.palette.accentPrimary, FontWeight.Bold))
        }
      }
      .width('100%')

      Text(opt.subtitle)
        .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
        .maxLines(1)
    }
    .alignItems(HorizontalAlign.Start)
    .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
    .padding(OrbitTokens.SPACE_MD)
    .layoutWeight(1)
    .backgroundColor(
      this.store.themeMode === opt.id
        ? this.store.palette.accentPrimarySoft
        : this.store.palette.elevated
    )
    .borderRadius(OrbitTokens.CARD_RADIUS)
    .border({
      width: OrbitTokens.HAIRLINE,
      color: this.store.themeMode === opt.id
        ? this.store.palette.accentPrimary
        : this.store.palette.hairline
    })
    .onClick(() => {
      this.store.setThemeMode(opt.id);
    })
  }

  build() {
    Column({ space: OrbitTokens.SPACE_LG }) {
      Row() {
        Text('Settings')
          .attributeModifier(orbitTextTitle(this.store.palette.fgPrimary, FontWeight.Bold))
        Blank()
        Button('Done')
          .attributeModifier(orbitButtonSubhead(this.store.palette.accentPrimary, Color.Transparent))
          .height(OrbitTokens.HIT_MIN)
          .onClick(() => {
            this.store.showHostSettingsSheet = false;
          })
      }
      .width('100%')

      Scroll() {
        Column({ space: OrbitTokens.SPACE_XL }) {
          if (this.store.clipboardPairingUri.length > 0) {
            Row({ space: OrbitTokens.SPACE_SM }) {
              Column({ space: 2 }) {
                Text('Pair from Clipboard')
                  .attributeModifier(
                    orbitTextSubhead(this.store.palette.accentPrimary, FontWeight.Bold)
                  )
                Text(this.store.clipboardPairingUri)
                  .attributeModifier(orbitTextMono(this.store.palette.fgSecondary))
                  .maxLines(1)
                  .textOverflow({ overflow: TextOverflow.Ellipsis })
              }
              .alignItems(HorizontalAlign.Start)
              .layoutWeight(1)

              Button('Pair')
                .attributeModifier(
                  orbitButtonSubhead(
                    this.store.palette.onAccent,
                    this.store.palette.accentPrimaryFill
                  )
                )
                .height(OrbitTokens.HIT_MIN)
                .borderRadius(OrbitTokens.PILL_RADIUS)
                .onClick(() => {
                  this.store.pairFromClipboard();
                })

              Stack({ alignContent: Alignment.Center }) {
                Text('×')
                  .attributeModifier(orbitTextSubhead(this.store.palette.fgMuted, FontWeight.Bold))
              }
              .width(OrbitTokens.HIT_MIN)
              .height(OrbitTokens.HIT_MIN)
              .accessibilityText('Dismiss clipboard pairing banner')
              .onClick(() => {
                this.store.dismissClipboardPairingBanner();
              })
            }
            .width('100%')
            .padding({
              left: OrbitTokens.SPACE_MD,
              right: OrbitTokens.SPACE_XS,
              top: OrbitTokens.SPACE_SM,
              bottom: OrbitTokens.SPACE_SM
            })
            .backgroundColor(this.store.palette.accentPrimarySoft)
            .borderRadius(OrbitTokens.CARD_RADIUS)
            .border({
              width: OrbitTokens.HAIRLINE,
              color: this.store.palette.accentPrimary
            })
            .alignItems(VerticalAlign.Center)
          }

          Column({ space: OrbitTokens.SPACE_SM }) {
            Text('APPEARANCE')
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Bold))

            Row({ space: OrbitTokens.SPACE_SM }) {
              ForEach(
                THEME_ROW_TOP,
                (opt: ThemeOptionItem) => {
                  this.renderThemeCard(opt)
                },
                (opt: ThemeOptionItem) => opt.id
              )
            }
            .width('100%')

            Row({ space: OrbitTokens.SPACE_SM }) {
              ForEach(
                THEME_ROW_BOTTOM,
                (opt: ThemeOptionItem) => {
                  this.renderThemeCard(opt)
                },
                (opt: ThemeOptionItem) => opt.id
              )
            }
            .width('100%')
          }
          .alignItems(HorizontalAlign.Start)
          .width('100%')

          Column({ space: OrbitTokens.SPACE_SM }) {
            Text('PAIR HOST')
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Bold))

            Button('Scan QR Code')
              .attributeModifier(
                orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
              )
              .width('100%')
              .height(OrbitTokens.HIT_MIN)
              .borderRadius(OrbitTokens.CARD_RADIUS)
              .onClick(() => {
                this.store.scanQrAndPair(this.getUIContext().getHostContext());
              })

            Row({ space: OrbitTokens.SPACE_SM }) {
              TextInput({
                placeholder: 'orbit://pair/...',
                text: this.pairingUriDraft
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
                .onChange((val: string) => {
                  this.pairingUriDraft = val;
                })

              PasteButton({
                icon: PasteIconStyle.LINES,
                text: PasteDescription.PASTE,
                buttonType: ButtonType.Capsule
              })
                .height(OrbitTokens.HIT_MIN)
                .borderRadius(OrbitTokens.CARD_RADIUS)
                .backgroundColor(this.store.palette.accentPrimarySoft)
                .fontColor(this.store.palette.accentPrimary)
                .iconColor(this.store.palette.accentPrimary)
                .onClick((_event: ClickEvent, result: PasteButtonOnClickResult) => {
                  if (result === PasteButtonOnClickResult.SUCCESS) {
                    this.store.pasteAuthorizedPairingUri((val: string) => {
                      this.pairingUriDraft = val;
                    });
                  }
                })

              Button('Pair Host')
                .attributeModifier(
                  orbitButtonSubhead(this.store.palette.fgPrimary, this.store.palette.subtleSurface)
                )
                .height(OrbitTokens.HIT_MIN)
                .borderRadius(OrbitTokens.CARD_RADIUS)
                .onClick(() => {
                  this.store.pairHost(this.pairingUriDraft);
                  this.pairingUriDraft = '';
                })
            }
            .width('100%')
          }
          .alignItems(HorizontalAlign.Start)
          .width('100%')

          if (this.activeHostCard() !== null) {
            Column({ space: OrbitTokens.SPACE_SM }) {
              Text(`HOST DIAGNOSTICS & ROUTING`)
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Bold))

              // Active Host Diagnostics Row with One-Tap "Reconnect" Action
              Column({ space: OrbitTokens.SPACE_XS }) {
                Row({ space: OrbitTokens.SPACE_SM }) {
                  Row()
                    .width(OrbitTokens.DOT_SIZE)
                    .height(OrbitTokens.DOT_SIZE)
                    .borderRadius(3)
                    .backgroundColor(this.hostDotColor(this.activeHostCard()!))

                  Column({ space: 2 }) {
                    Text(this.activeHostCard()?.display_name ?? '')
                      .attributeModifier(
                        orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold)
                      )
                      .maxLines(1)
                      .textOverflow({ overflow: TextOverflow.Ellipsis })
                    Text(this.hostDiagnosticsSubtitle(this.activeHostCard()!))
                      .attributeModifier(orbitTextMono(this.store.palette.fgSecondary))
                      .maxLines(1)
                      .textOverflow({ overflow: TextOverflow.Ellipsis })
                  }
                  .alignItems(HorizontalAlign.Start)
                  .layoutWeight(1)

                  Button('Reconnect')
                    .attributeModifier(
                      orbitButtonMeta(
                        this.store.palette.accentPrimary,
                        this.store.palette.accentPrimarySoft
                      )
                    )
                    .height(32)
                    .borderRadius(OrbitTokens.PILL_RADIUS)
                    .onClick(() => {
                      const host = this.activeHostCard();
                      if (host !== null) {
                        this.store.reconnectHost(host.host_id);
                      }
                    })
                }
                .width('100%')
                .alignItems(VerticalAlign.Center)

                if (
                  this.activeHostCard()?.last_error !== null &&
                  (this.activeHostCard()?.last_error ?? '').length > 0
                ) {
                  Text(this.activeHostCard()?.last_error ?? '')
                    .attributeModifier(orbitTextMeta(this.store.palette.statusDanger))
                    .maxLines(2)
                    .textOverflow({ overflow: TextOverflow.Ellipsis })
                    .width('100%')
                }
              }
              .width('100%')
              .padding(OrbitTokens.SPACE_MD)
              .backgroundColor(this.store.palette.elevated)
              .borderRadius(OrbitTokens.CARD_RADIUS)
              .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })

              Flex({ wrap: FlexWrap.Wrap }) {
                ForEach(
                  ROUTE_MODES,
                  (rm: string) => {
                    Button(formatRouteModeLabel(rm))
                      .attributeModifier(
                        orbitButtonSubhead(
                          this.routeModeDraft === rm
                            ? this.store.palette.onAccent
                            : this.store.palette.fgPrimary,
                          this.routeModeDraft === rm
                            ? this.store.palette.accentPrimaryFill
                            : this.store.palette.subtleSurface
                        )
                      )
                      .height(OrbitTokens.HIT_MIN)
                      .borderRadius(OrbitTokens.PILL_RADIUS)
                      .margin({ right: OrbitTokens.SPACE_SM, bottom: OrbitTokens.SPACE_SM })
                      .onClick(() => {
                        this.routeModeDraft = rm;
                      })
                  },
                  (rm: string) => rm
                )
              }

              TextInput({
                placeholder: 'Custom display name (optional)',
                text: this.customNameDraft
              })
                .attributeModifier(
                  orbitInputSubhead(
                    this.store.palette.fgPrimary,
                    this.store.palette.fgMuted,
                    this.store.palette.elevated
                  )
                )
                .height(OrbitTokens.HIT_MIN)
                .borderRadius(OrbitTokens.CARD_RADIUS)
                .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
                .width('100%')
                .onChange((v: string) => {
                  this.customNameDraft = v;
                })

              TextInput({
                placeholder: 'Custom Direct Endpoint (ws://ip:6868/ws)',
                text: this.directEndpointDraft
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
                .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
                .width('100%')
                .onChange((v: string) => {
                  this.directEndpointDraft = v;
                })

              TextInput({
                placeholder: 'Noise E2EE Relay URL (wss://relay.example.com)',
                text: this.relayUrlDraft
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
                .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
                .width('100%')
                .onChange((v: string) => {
                  this.relayUrlDraft = v;
                })

              if (this.routeModeDraft === 'ssh_tunnel') {
                Row({ space: OrbitTokens.SPACE_SM }) {
                  TextInput({
                    placeholder: 'SSH host alias',
                    text: this.sshAliasDraft
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
                    .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
                    .layoutWeight(2)
                    .onChange((v: string) => {
                      this.sshAliasDraft = v;
                    })

                  TextInput({
                    placeholder: '6868',
                    text: this.portDraft
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
                    .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
                    .layoutWeight(1)
                    .onChange((v: string) => {
                      this.portDraft = v;
                    })
                }
                .width('100%')
              }

              Row({ space: OrbitTokens.SPACE_SM }) {
                Button('Save & Reconnect')
                  .attributeModifier(
                    orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
                  )
                  .height(OrbitTokens.HIT_MIN)
                  .borderRadius(OrbitTokens.CARD_RADIUS)
                  .layoutWeight(1)
                  .onClick(() => {
                    const host = this.activeHostCard();
                    if (host !== null) {
                      const portNum = parseInt(this.portDraft, 10);
                      this.store.updateHostConfig(
                        host.host_id,
                        this.customNameDraft,
                        this.routeModeDraft,
                        this.sshAliasDraft,
                        Number.isNaN(portNum) ? 6868 : portNum,
                        this.directEndpointDraft,
                        this.relayUrlDraft
                      );
                    }
                  })

                Button('Remove Host')
                  .attributeModifier(
                    orbitButtonSubhead(
                      this.store.palette.statusDanger,
                      this.store.palette.statusDangerSoft
                    )
                  )
                  .height(OrbitTokens.HIT_MIN)
                  .borderRadius(OrbitTokens.CARD_RADIUS)
                  .onClick(() => {
                    const host = this.activeHostCard();
                    if (host !== null) {
                      this.store.removeHost(host.host_id);
                    }
                  })
              }
              .width('100%')
            }
            .alignItems(HorizontalAlign.Start)
            .width('100%')
          }

          Column({ space: OrbitTokens.SPACE_SM }) {
            Row() {
              Text('ACCOUNT QUOTAS')
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Bold))
              Blank()
              Button('Refresh')
                .attributeModifier(
                  orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.subtleSurface)
                )
                .height(32)
                .borderRadius(OrbitTokens.PILL_RADIUS)
                .onClick(() => {
                  this.store.refreshActiveQuotas();
                })
            }
            .width('100%')

            if (this.quotas().length === 0) {
              Text('No quota snapshots reported by connected providers yet.')
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
            } else {
              ForEach(
                this.quotas(),
                (q: ProviderQuotaDto) => {
                  Column({ space: OrbitTokens.SPACE_SM }) {
                    Row() {
                      Text(formatProviderLabel(q.provider))
                        .attributeModifier(
                          orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold)
                        )
                      Blank()
                      if (q.account_label !== null && q.account_label.length > 0) {
                        Text(q.account_label)
                          .attributeModifier(orbitTextMono(this.store.palette.fgSecondary))
                      }
                    }
                    .width('100%')

                    if (q.error !== null && q.error.length > 0) {
                      Text(q.error)
                        .attributeModifier(orbitTextMeta(this.store.palette.statusDanger))
                        .width('100%')
                    }

                    ForEach(
                      q.windows,
                      (win: QuotaWindowDto) => {
                        Column({ space: OrbitTokens.SPACE_XS }) {
                          Row({ space: OrbitTokens.SPACE_SM }) {
                            Text(win.label)
                              .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                              .layoutWeight(1)
                              .maxLines(1)
                              .textOverflow({ overflow: TextOverflow.Ellipsis })

                            if (formatQuotaResetCountdown(win.resets_at).length > 0) {
                              Text(formatQuotaResetCountdown(win.resets_at))
                                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                                .maxLines(1)
                            }

                            Text(`${Math.round(win.used_percent)}%`)
                              .attributeModifier(
                                orbitTextMono(
                                  this.quotaSemanticColor(win.used_percent),
                                  FontWeight.Medium
                                )
                              )
                          }
                          .width('100%')
                          .alignItems(VerticalAlign.Center)

                          // 4vp-tall semantic progress bar (<70% statusDirect, 70-90% statusWarning, >90% statusDanger)
                          Stack({ alignContent: Alignment.Start }) {
                            Row()
                              .width('100%')
                              .height(OrbitTokens.SPACE_XS)
                              .backgroundColor(this.store.palette.subtleSurface)
                              .borderRadius(2)
                            Row()
                              .width(`${Math.min(100, Math.max(0, Math.round(win.used_percent)))}%`)
                              .height(OrbitTokens.SPACE_XS)
                              .backgroundColor(this.quotaSemanticColor(win.used_percent))
                              .borderRadius(2)
                          }
                          .width('100%')
                        }
                        .width('100%')
                      },
                      (win: QuotaWindowDto) => win.label
                    )

                    if (q.credits_remaining !== null) {
                      Text(`Credits remaining: ${q.credits_remaining.toFixed(2)}`)
                        .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                    }
                  }
                  .width('100%')
                  .padding(OrbitTokens.SPACE_MD)
                  .backgroundColor(this.store.palette.elevated)
                  .borderRadius(OrbitTokens.CARD_RADIUS)
                  .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
                },
                (q: ProviderQuotaDto) => q.provider
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
