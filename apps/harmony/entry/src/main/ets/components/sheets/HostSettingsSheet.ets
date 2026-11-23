import {
  CapsuleSegmentButtonV2,
  LengthMetrics,
  SegmentButtonV2Items
} from '@kit.ArkUI';
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
  orbitSegmentSelectedBg,
  orbitSegmentSelectedFg,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import {
  OrbitSectionHeader,
  dismissHarmonyKeyboard,
  resolveHostStatusColor
} from '../common/OrbitCommonControls';
import { OrbitProviderBrandIcon } from '../composer/ModelPickerSheet';

const APPEARANCE_MODES: string[] = ['system', 'light', 'dark'];
const ROUTE_MODES: string[] = ['auto', 'direct_only', 'relay_only'];

@ComponentV2
export struct HostSettingsSheet {
  @Local pairUriInput: string = '';
  @Local inspectedHostId: string = '';
  @Local customNameDraft: string = '';
  @Local routeModeDraft: string = 'auto';
  @Local directEndpointDraft: string = '';
  @Local relayUrlDraft: string = '';
  @Local cloudTokenDraft: string = '';
  @Local cloudPassphraseDraft: string = '';
  @Local appearanceItems: SegmentButtonV2Items = new SegmentButtonV2Items([
    { text: 'System' },
    { text: 'Light' },
    { text: 'Dark' }
  ]);
  @Local routeModeItems: SegmentButtonV2Items = new SegmentButtonV2Items([
    { text: formatRouteModeLabel('auto') },
    { text: formatRouteModeLabel('direct_only') },
    { text: formatRouteModeLabel('relay_only') }
  ]);

  private navPathStack: NavPathStack = new NavPathStack();
  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  @Computed
  get appearanceIndex(): number {
    const mode = this.store.themeMode;
    if (mode === 'light') {
      return 1;
    }
    if (mode === 'dark' || mode === 'graphite' || mode === 'oled') {
      return 2;
    }
    return 0;
  }

  @Computed
  get routeModeIndex(): number {
    return ROUTE_MODES.indexOf(this.routeModeDraft);
  }

  @Computed
  get quotas(): ProviderQuotaDto[] {
    const detail = this.store.currentHostDetail;
    return detail !== null ? detail.quotas : [];
  }

  @Computed
  get inspectedHost(): HostCardDto | null {
    for (let i = 0; i < this.store.hosts.length; i++) {
      if (this.store.hosts[i].host_id === this.inspectedHostId) {
        return this.store.hosts[i];
      }
    }
    return null;
  }

  private pushHostDetail(host: HostCardDto): void {
    this.inspectedHostId = host.host_id;
    this.customNameDraft = host.display_name;
    this.routeModeDraft = host.route_mode;
    this.directEndpointDraft = host.custom_direct_endpoint ?? '';
    this.relayUrlDraft = host.relay_url ?? '';
    this.navPathStack.pushPathByName('host_detail', null);
    this.store.triggerHaptic(6);
  }

  private hostSummarySubtitle(host: HostCardDto): string {
    const state = formatConnectionStateLabel(host.connection_state, host.via_ssh_tunnel);
    return host.last_ping_ms !== null ? `${state} · ${host.last_ping_ms} ms` : state;
  }

  private daemonSummary(host: HostCardDto): string {
    const parts: string[] = [];
    if (host.daemon_version !== null && host.daemon_version.length > 0) {
      parts.push(host.daemon_version.startsWith('v') ? host.daemon_version : `v${host.daemon_version}`);
    }
    if (host.os_arch !== null && host.os_arch.length > 0) {
      parts.push(host.os_arch);
    }
    return parts.length > 0 ? parts.join(' · ') : '—';
  }

  private quotaSemanticColor(usedPercent: number): ResourceColor {
    if (usedPercent > 90) {
      return this.store.palette.statusDanger;
    }
    if (usedPercent >= 70) {
      return this.store.palette.statusWarning;
    }
    return this.store.palette.statusDirect;
  }

  private confirmRemoveHost(host: HostCardDto): void {
    this.getUIContext().showAlertDialog({
      title: `Remove ${host.display_name}?`,
      message: 'Running remote agent sessions on this host will not be terminated.',
      autoCancel: true,
      primaryButton: {
        value: 'Cancel',
        fontColor: this.store.palette.fgSecondary,
        action: () => {
          // Dismissed
        }
      },
      secondaryButton: {
        value: 'Remove',
        fontColor: this.store.palette.statusDanger,
        action: () => {
          const removingLast = this.store.hosts.length <= 1;
          this.store.removeHost(host.host_id);
          this.inspectedHostId = '';
          this.navPathStack.pop();
          if (removingLast) {
            this.store.showHostSettingsSheet = false;
          }
        }
      }
    });
  }

  aboutToAppear(): void {
    this.store.refreshCloudSyncStatus();
    this.store.probeNotificationPermission();
    if (this.store.selectedHostId.length > 0) {
      this.store.refreshActiveQuotas();
    }
  }

  private cloudSyncSummaryLabel(): string {
    const st = this.store.cloudSyncStatus;
    if (st === null || st.state === 'signed_out') {
      return 'Not connected';
    }
    if (st.state === 'locked') {
      return st.account_login !== null ? `@${st.account_login} · Locked` : 'Locked';
    }
    if (st.state === 'syncing') {
      return 'Syncing…';
    }
    if (st.state === 'error') {
      return 'Sync error';
    }
    const login = st.account_login !== null ? `@${st.account_login}` : 'Connected';
    return `${login} · Rev ${st.last_vault_revision}`;
  }

  private confirmDisconnectCloudSync(): void {
    this.getUIContext().showAlertDialog({
      title: 'Disconnect Cloud Sync?',
      message: 'Your encrypted GitHub Gist vault remains intact, and local hosts are preserved.',
      autoCancel: true,
      primaryButton: {
        value: 'Cancel',
        fontColor: this.store.palette.fgSecondary,
        action: () => {
          // Dismissed
        }
      },
      secondaryButton: {
        value: 'Disconnect',
        fontColor: this.store.palette.statusDanger,
        action: () => {
          this.store.cloudSyncDisconnect(false);
        }
      }
    });
  }

  @Builder
  private renderQuotasHeader() {
    Row() {
      Text('Usage & Quotas')
        .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Medium))
        .maxLines(1)
      Blank()
      Button({ type: ButtonType.Normal, stateEffect: true }) {
        Row({ space: 4 }) {
          SymbolGlyph($r('sys.symbol.arrow_clockwise'))
            .fontSize(13)
            .fontColor([this.store.palette.accentPrimary])
          Text('Refresh')
            .attributeModifier(orbitTextMeta(this.store.palette.accentPrimary, FontWeight.Medium))
        }
      }
      .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
      .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_XS })
      .backgroundColor(Color.Transparent)
      .onClick(() => {
        this.store.refreshActiveQuotas();
        this.store.triggerHaptic(6);
      })
    }
    .width('100%')
    .padding({
      left: OrbitTokens.SPACE_XS,
      right: 0,
      top: 14,
      bottom: 2
    })
    .alignItems(VerticalAlign.Center)
  }

  @Builder
  private renderRootSettings() {
    List({ space: 0 }) {
      // 1. Appearance (Native HarmonyOS 7 CapsuleSegmentButtonV2)
      ListItem() {
        OrbitSectionHeader({ title: 'Appearance', isFirst: true })
      }

      ListItem() {
        CapsuleSegmentButtonV2({
          items: this.appearanceItems,
          selectedIndex: this.appearanceIndex,
          onItemClicked: (idx: number) => {
            if (idx >= 0 && idx < APPEARANCE_MODES.length) {
              this.store.setThemeMode(APPEARANCE_MODES[idx]);
            }
          },
          itemSelectedBackgroundColor: orbitSegmentSelectedBg(this.store.palette),
          itemSelectedFontColor: orbitSegmentSelectedFg(this.store.palette),
          itemMaxFontScale: 1.45,
          itemMinHeight: LengthMetrics.vp(38)
        })
          .width('100%')
      }

      // 2. Hosts Fleet
      ListItem() {
        OrbitSectionHeader({ title: 'Hosts' })
      }

      ListItemGroup() {
        if (this.store.hosts.length === 0) {
          ListItem() {
            Row() {
              Text('No hosts paired yet')
                .attributeModifier(orbitTextSubhead(this.store.palette.fgMuted))
            }
            .width('100%')
            .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
            .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
            .alignItems(VerticalAlign.Center)
          }
        } else {
          ForEach(
            this.store.hosts,
            (host: HostCardDto) => {
              ListItem() {
                Button({ type: ButtonType.Normal, stateEffect: true }) {
                  Row({ space: 10 }) {
                    Row() {
                      Row()
                        .width(8)
                        .height(8)
                        .borderRadius(4)
                        .backgroundColor(resolveHostStatusColor(host, this.store))
                    }
                    .width(20)
                    .justifyContent(FlexAlign.Center)

                    Column({ space: 2 }) {
                      Row({ space: 6 }) {
                        Text(host.display_name)
                          .attributeModifier(
                            orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium)
                          )
                          .maxLines(1)
                          .textOverflow({ overflow: TextOverflow.Ellipsis })
                          .flexShrink(1)

                        if (host.host_id === this.store.selectedHostId && this.store.hosts.length > 1) {
                          Text('Active')
                            .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                            .padding({ left: 6, right: 6, top: 1, bottom: 1 })
                            .borderRadius(OrbitTokens.RADIUS_SM)
                            .backgroundColor(this.store.palette.subtleSurface)
                        }
                      }
                      .alignItems(VerticalAlign.Center)

                      Text(this.hostSummarySubtitle(host))
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
                  .alignItems(VerticalAlign.Center)
                }
                .width('100%')
                .constraintSize({ minHeight: 56 })
                .padding({
                  left: OrbitTokens.SPACE_LG,
                  right: OrbitTokens.SPACE_LG,
                  top: 8,
                  bottom: 8
                })
                .backgroundColor(Color.Transparent)
                .onClick(() => {
                  this.pushHostDetail(host);
                })
              }
            },
            (host: HostCardDto) => `host_row_${host.host_id}`
          )
        }

        ListItem() {
          Button({ type: ButtonType.Normal, stateEffect: true }) {
            Row({ space: 10 }) {
              Row() {
                SymbolGlyph($r('sys.symbol.qrcode'))
                  .fontSize(16)
                  .fontColor([this.store.palette.accentPrimary])
              }
              .width(20)
              .justifyContent(FlexAlign.Center)

              Text('Pair New Host')
                .attributeModifier(
                  orbitTextSubhead(this.store.palette.accentPrimary, FontWeight.Medium)
                )
                .layoutWeight(1)
                .textAlign(TextAlign.Start)

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
            this.navPathStack.pushPathByName('pair_host', null);
            this.store.triggerHaptic(6);
          })
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

      // 3. Usage & Quotas
      if (this.store.selectedHostId.length > 0) {
        ListItem() {
          this.renderQuotasHeader()
        }

        ListItemGroup() {
          if (this.quotas.length === 0) {
            ListItem() {
              Row() {
                Text('No active rate-limit windows reported yet.')
                  .attributeModifier(orbitTextSubhead(this.store.palette.fgMuted))
              }
              .width('100%')
              .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
              .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
              .alignItems(VerticalAlign.Center)
            }
          } else {
            ForEach(
              this.quotas,
              (q: ProviderQuotaDto) => {
                ListItem() {
                  Column({ space: OrbitTokens.SPACE_SM }) {
                    Row({ space: OrbitTokens.SPACE_SM }) {
                      OrbitProviderBrandIcon({ provider: q.provider, iconSize: 16 })
                      Text(formatProviderLabel(q.provider))
                        .attributeModifier(
                          orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium)
                        )
                      if (q.account_label !== null && q.account_label.length > 0) {
                        Text(q.account_label)
                          .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                          .maxLines(1)
                          .textOverflow({ overflow: TextOverflow.Ellipsis })
                          .flexShrink(1)
                      }
                      Blank()
                      if (q.credits_remaining !== null) {
                        Text(`$${q.credits_remaining.toFixed(2)}`)
                          .attributeModifier(
                            orbitTextMono(this.store.palette.statusDirect, FontWeight.Medium)
                          )
                      }
                    }
                    .width('100%')
                    .alignItems(VerticalAlign.Center)

                    if (q.error !== null && q.error.length > 0) {
                      Text(q.error)
                        .attributeModifier(orbitTextMeta(this.store.palette.statusWarning))
                        .width('100%')
                    }

                    if (q.windows.length === 0 && (q.error === null || q.error.length === 0)) {
                      Text('No active rate-limit windows')
                        .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                        .width('100%')
                    } else {
                      ForEach(
                        q.windows,
                        (win: QuotaWindowDto) => {
                          Column({ space: 5 }) {
                            Row({ space: OrbitTokens.SPACE_SM }) {
                              Text(win.label)
                                .attributeModifier(
                                  orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium)
                                )
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

                            Progress({
                              value: Math.min(100, Math.max(0, Math.round(win.used_percent))),
                              total: 100,
                              type: ProgressType.Linear
                            })
                              .width('100%')
                              .color(this.quotaSemanticColor(win.used_percent))
                              .backgroundColor(this.store.palette.subtleSurface)
                              .style({ strokeWidth: 4, enableSmoothEffect: true })
                          }
                          .width('100%')
                        },
                        (win: QuotaWindowDto) => win.label
                      )
                    }
                  }
                  .width('100%')
                  .padding(OrbitTokens.SPACE_LG)
                }
              },
              (q: ProviderQuotaDto) => q.provider
            )
          }
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

      // 4. Cloud Sync (GitHub Gist E2EE) & Push Notifications
      ListItem() {
        OrbitSectionHeader({ title: 'Sync & Alerts' })
      }

      ListItemGroup() {
        ListItem() {
          Button({ type: ButtonType.Normal, stateEffect: true }) {
            Row({ space: 10 }) {
              Row() {
                SymbolGlyph($r('sys.symbol.cloud'))
                  .fontSize(16)
                  .fontColor([this.store.palette.iconPrimary])
              }
              .width(20)
              .justifyContent(FlexAlign.Center)

              Column({ space: 2 }) {
                Text('Cloud Sync (E2EE)')
                  .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                Text(this.cloudSyncSummaryLabel())
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
            .alignItems(VerticalAlign.Center)
          }
          .width('100%')
          .constraintSize({ minHeight: 56 })
          .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG, top: 8, bottom: 8 })
          .backgroundColor(Color.Transparent)
          .onClick(() => {
            this.store.refreshCloudSyncStatus();
            this.navPathStack.pushPathByName('cloud_sync', null);
            this.store.triggerHaptic(6);
          })
        }

        ListItem() {
          Button({ type: ButtonType.Normal, stateEffect: true }) {
            Row({ space: 10 }) {
              Row() {
                SymbolGlyph($r('sys.symbol.bell'))
                  .fontSize(16)
                  .fontColor([this.store.palette.iconPrimary])
              }
              .width(20)
              .justifyContent(FlexAlign.Center)

              Column({ space: 2 }) {
                Text('Push Kit & Notifications')
                  .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                Text(
                  this.store.pushToken.length > 0
                    ? `Token ${this.store.pushToken.substring(0, 12)}…`
                    : (this.store.pushStatusMessage.length > 0
                      ? this.store.pushStatusMessage
                      : 'Tap to register Push Kit token')
                )
                  .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                  .maxLines(1)
                  .textOverflow({ overflow: TextOverflow.Ellipsis })
              }
              .alignItems(HorizontalAlign.Start)
              .layoutWeight(1)

              Text(this.store.notificationsEnabled ? 'Enabled' : 'Enable')
                .attributeModifier(orbitTextMeta(this.store.palette.accentPrimary, FontWeight.Medium))
            }
            .width('100%')
            .alignItems(VerticalAlign.Center)
          }
          .width('100%')
          .constraintSize({ minHeight: 56 })
          .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG, top: 8, bottom: 8 })
          .backgroundColor(Color.Transparent)
          .onClick(() => {
            this.store.configurePushAndNotifications(this.getUIContext().getHostContext());
          })
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

      // 5. About
      ListItem() {
        OrbitSectionHeader({ title: 'About' })
      }

      ListItemGroup() {
        ListItem() {
          Row() {
            Text('Encryption')
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
            Blank()
            Text('Noise NK · E2EE')
              .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
          }
          .width('100%')
          .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
          .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
          .alignItems(VerticalAlign.Center)
        }

        ListItem() {
          Row() {
            Text('Platform')
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
            Blank()
            Text('HarmonyOS 7 · ArkUI V2')
              .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
          }
          .width('100%')
          .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
          .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
          .alignItems(VerticalAlign.Center)
        }
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
    .width('100%')
    .height('100%')
    .padding({
      left: OrbitTokens.SPACE_LG,
      right: OrbitTokens.SPACE_LG,
      top: OrbitTokens.SPACE_XS,
      bottom: Math.max(OrbitTokens.SPACE_XL, this.store.safeBottomVp)
    })
    .scrollBar(BarState.Auto)
    .edgeEffect(EdgeEffect.Spring)
    .onScrollStart(() => {
      dismissHarmonyKeyboard(this.getUIContext());
    })
  }

  @Builder
  private renderHostDetailDestination(host: HostCardDto) {
    NavDestination() {
      List({ space: 0 }) {
        // 1. Diagnostics Group
        ListItem() {
          OrbitSectionHeader({ title: 'Diagnostics', isFirst: true })
        }

        ListItemGroup() {
          ListItem() {
            Row({ space: OrbitTokens.SPACE_SM }) {
              Row()
                .width(8)
                .height(8)
                .borderRadius(4)
                .backgroundColor(resolveHostStatusColor(host, this.store))

              Text(formatConnectionStateLabel(host.connection_state, host.via_ssh_tunnel))
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                .layoutWeight(1)

              Button('Reconnect', { type: ButtonType.Capsule, stateEffect: true })
                .attributeModifier(
                  orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.subtleSurface)
                )
                .height(OrbitTokens.HIT_COMPACT)
                .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
                .onClick(() => {
                  this.store.reconnectHost(host.host_id);
                })
            }
            .width('100%')
            .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
            .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_MD })
            .alignItems(VerticalAlign.Center)
          }

          ListItem() {
            Row() {
              Text('Latency')
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
              Blank()
              Text(host.last_ping_ms !== null ? `${host.last_ping_ms} ms` : '—')
                .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
            }
            .width('100%')
            .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
            .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
            .alignItems(VerticalAlign.Center)
          }

          ListItem() {
            Row() {
              Text('Daemon')
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
              Blank()
              Text(this.daemonSummary(host))
                .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                .maxLines(1)
                .textOverflow({ overflow: TextOverflow.Ellipsis })
            }
            .width('100%')
            .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
            .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
            .alignItems(VerticalAlign.Center)
          }

          ListItem() {
            Button({ type: ButtonType.Normal, stateEffect: true }) {
              Row({ space: 6 }) {
                Text('Public Key')
                  .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                Blank()
                Text(`${host.daemon_pubkey_b64.substring(0, 16)}…`)
                  .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                  .maxLines(1)
                SymbolGlyph($r('sys.symbol.square_and_square'))
                  .fontSize(14)
                  .fontColor([this.store.palette.iconSecondary])
              }
              .width('100%')
              .alignItems(VerticalAlign.Center)
            }
            .width('100%')
            .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
            .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
            .backgroundColor(Color.Transparent)
            .onClick(() => {
              this.store.copyToClipboard(host.daemon_pubkey_b64, 'Copied public key');
            })
          }

          if (host.last_error !== null && host.last_error.length > 0) {
            ListItem() {
              Text(host.last_error)
                .attributeModifier(orbitTextMono(this.store.palette.statusDanger))
                .copyOption(CopyOptions.LocalDevice)
                .width('100%')
                .padding({
                  left: OrbitTokens.SPACE_LG,
                  right: OrbitTokens.SPACE_LG,
                  top: OrbitTokens.SPACE_SM,
                  bottom: OrbitTokens.SPACE_SM
                })
            }
          }

          if (host.host_id !== this.store.selectedHostId) {
            ListItem() {
              Button('Switch to This Host', { type: ButtonType.Normal, stateEffect: true })
                .attributeModifier(orbitButtonSubhead(this.store.palette.accentPrimary, Color.Transparent))
                .width('100%')
                .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                .onClick(() => {
                  this.store.selectHost(host.host_id);
                  this.navPathStack.pop();
                  this.store.triggerHaptic(8);
                })
            }
          }
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

        // 2. Connection Group (Native CapsuleSegmentButtonV2 Route Mode)
        ListItem() {
          OrbitSectionHeader({ title: 'Connection' })
        }

        ListItemGroup() {
          ListItem() {
            Column({ space: OrbitTokens.SPACE_SM }) {
              Text('Route Mode')
                .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium))

              if (this.routeModeIndex >= 0) {
                CapsuleSegmentButtonV2({
                  items: this.routeModeItems,
                  selectedIndex: this.routeModeIndex,
                  onItemClicked: (idx: number) => {
                    if (idx >= 0 && idx < ROUTE_MODES.length) {
                      this.routeModeDraft = ROUTE_MODES[idx];
                      this.store.triggerHaptic(6);
                    }
                  },
                  itemFontSize: LengthMetrics.fp(OrbitTokens.FONT_META),
                  itemSelectedFontSize: LengthMetrics.fp(OrbitTokens.FONT_META),
                  itemSelectedBackgroundColor: orbitSegmentSelectedBg(this.store.palette),
                  itemSelectedFontColor: orbitSegmentSelectedFg(this.store.palette),
                  itemMaxFontScale: 1.3,
                  itemMinHeight: LengthMetrics.vp(36),
                  itemPadding: {
                    top: LengthMetrics.vp(4),
                    bottom: LengthMetrics.vp(4),
                    start: LengthMetrics.vp(4),
                    end: LengthMetrics.vp(4)
                  }
                })
                  .width('100%')
              } else {
                Column({ space: OrbitTokens.SPACE_XS }) {
                  Text(`${formatRouteModeLabel(this.routeModeDraft)} (preserved unless changed)`)
                    .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))

                  Row({ space: OrbitTokens.SPACE_XS }) {
                    ForEach(
                      ROUTE_MODES,
                      (mode: string) => {
                        Button(formatRouteModeLabel(mode), { type: ButtonType.Capsule, stateEffect: true })
                          .attributeModifier(
                            orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface)
                          )
                          .height(36)
                          .layoutWeight(1)
                          .onClick(() => {
                            this.routeModeDraft = mode;
                            this.store.triggerHaptic(6);
                          })
                      },
                      (mode: string) => mode
                    )
                  }
                  .width('100%')
                }
                .alignItems(HorizontalAlign.Start)
                .width('100%')
              }
            }
            .alignItems(HorizontalAlign.Start)
            .width('100%')
            .padding(OrbitTokens.SPACE_LG)
          }

          ListItem() {
            Column({ space: 4 }) {
              Text('Display Name')
                .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium))
              TextInput({
                placeholder: host.hostname,
                text: this.customNameDraft
              })
                .attributeModifier(
                  orbitInputSubhead(
                    this.store.palette.fgPrimary,
                    this.store.palette.fgMuted,
                    Color.Transparent
                  )
                )
                .height(36)
                .padding(0)
                .width('100%')
                .onChange((v: string) => {
                  this.customNameDraft = v;
                })
            }
            .alignItems(HorizontalAlign.Start)
            .width('100%')
            .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG, top: 10, bottom: 10 })
          }

          ListItem() {
            Column({ space: 4 }) {
              Text('Direct Endpoint')
                .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium))
              TextInput({
                placeholder: host.direct_endpoints.length > 0 ? host.direct_endpoints[0] : 'ws://192.168.1.x:6868/ws',
                text: this.directEndpointDraft
              })
                .attributeModifier(
                  orbitInputMono(
                    this.store.palette.fgPrimary,
                    this.store.palette.fgMuted,
                    Color.Transparent
                  )
                )
                .height(36)
                .padding(0)
                .width('100%')
                .onChange((v: string) => {
                  this.directEndpointDraft = v;
                })
            }
            .alignItems(HorizontalAlign.Start)
            .width('100%')
            .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG, top: 10, bottom: 10 })
          }

          ListItem() {
            Column({ space: 4 }) {
              Text('Relay URL')
                .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium))
              TextInput({
                placeholder: 'wss://relay.orbit.dev',
                text: this.relayUrlDraft
              })
                .attributeModifier(
                  orbitInputMono(
                    this.store.palette.fgPrimary,
                    this.store.palette.fgMuted,
                    Color.Transparent
                  )
                )
                .height(36)
                .padding(0)
                .width('100%')
                .onChange((v: string) => {
                  this.relayUrlDraft = v;
                })
            }
            .alignItems(HorizontalAlign.Start)
            .width('100%')
            .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG, top: 10, bottom: 10 })
          }

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

        // Save & Reconnect Primary Action
        ListItem() {
          Button('Save & Reconnect')
            .attributeModifier(
              orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
            )
            .width('100%')
            .height(OrbitTokens.HIT_MIN)
            .borderRadius(OrbitTokens.RADIUS_LG)
            .onClick(() => {
              dismissHarmonyKeyboard(this.getUIContext());
              this.store.updateHostConfig(
                host.host_id,
                this.customNameDraft,
                this.routeModeDraft,
                host.ssh_alias ?? '',
                host.remote_daemon_port > 0 ? host.remote_daemon_port : 6868,
                this.directEndpointDraft,
                this.relayUrlDraft
              );
            })
        }
        .margin({ top: OrbitTokens.SPACE_LG })

        // 3. Danger Zone (Native Button)
        ListItem() {
          Button('Remove Host', { type: ButtonType.Normal, stateEffect: true })
            .attributeModifier(orbitButtonSubhead(this.store.palette.statusDanger, this.store.palette.listCard))
            .width('100%')
            .height(OrbitTokens.HIT_MIN)
            .borderRadius(OrbitTokens.RADIUS_LG)
            .onClick(() => {
              dismissHarmonyKeyboard(this.getUIContext());
              this.confirmRemoveHost(host);
            })
        }
        .margin({ top: OrbitTokens.SPACE_MD })
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
    .title(host.display_name)
    .backgroundColor(this.store.palette.drawer)
  }

  @Builder
  private renderPairHostDestination() {
    NavDestination() {
      List({ space: OrbitTokens.SPACE_LG }) {
        ListItem() {
          Button('Scan QR Code')
            .attributeModifier(
              orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
            )
            .width('100%')
            .height(OrbitTokens.HIT_MIN)
            .borderRadius(OrbitTokens.RADIUS_LG)
            .onClick(() => {
              this.store.scanQrAndPair(this.getUIContext().getHostContext());
            })
        }

        ListItem() {
          Column({ space: OrbitTokens.SPACE_SM }) {
            Text('Pairing URI')
              .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium))

            Row({ space: OrbitTokens.SPACE_SM }) {
              TextInput({
                placeholder: 'orbit://pair/...',
                text: this.pairUriInput
              })
                .attributeModifier(
                  orbitInputMono(
                    this.store.palette.fgPrimary,
                    this.store.palette.fgMuted,
                    Color.Transparent
                  )
                )
                .height(OrbitTokens.HIT_COMPACT)
                .padding(0)
                .layoutWeight(1)
                .onChange((val: string) => {
                  this.pairUriInput = val;
                })
                .onSubmit(() => {
                  if (this.pairUriInput.trim().length > 0) {
                    this.store.pairHost(this.pairUriInput);
                    this.pairUriInput = '';
                    this.navPathStack.pop();
                  }
                })

              PasteButton({
                icon: PasteIconStyle.LINES,
                text: PasteDescription.PASTE,
                buttonType: ButtonType.Capsule
              })
                .height(OrbitTokens.HIT_COMPACT)
                .backgroundColor(this.store.palette.subtleSurface)
                .fontColor(this.store.palette.fgPrimary)
                .iconColor(this.store.palette.iconPrimary)
                .onClick((_event: ClickEvent, result: PasteButtonOnClickResult) => {
                  if (result === PasteButtonOnClickResult.SUCCESS) {
                    this.store.pasteAuthorizedPairingUri((filled: string) => {
                      this.pairUriInput = filled;
                    });
                  }
                })
            }
            .width('100%')
            .alignItems(VerticalAlign.Center)
          }
          .alignItems(HorizontalAlign.Start)
          .width('100%')
          .padding(OrbitTokens.SPACE_LG)
          .backgroundColor(this.store.palette.listCard)
          .borderRadius(OrbitTokens.RADIUS_LG)
        }

        ListItem() {
          Button('Pair Host')
            .attributeModifier(
              orbitButtonSubhead(this.store.palette.fgPrimary, this.store.palette.listCard)
            )
            .width('100%')
            .height(OrbitTokens.HIT_MIN)
            .borderRadius(OrbitTokens.RADIUS_LG)
            .enabled(this.pairUriInput.trim().length > 0)
            .opacity(this.pairUriInput.trim().length > 0 ? 1.0 : 0.45)
            .onClick(() => {
              dismissHarmonyKeyboard(this.getUIContext());
              this.store.pairHost(this.pairUriInput);
              this.pairUriInput = '';
              this.navPathStack.pop();
            })
        }
      }
      .width('100%')
      .height('100%')
      .padding({
        left: OrbitTokens.SPACE_LG,
        right: OrbitTokens.SPACE_LG,
        top: OrbitTokens.SPACE_MD,
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
    .title('Pair Host')
    .backgroundColor(this.store.palette.drawer)
  }

  @Builder
  private renderCloudSyncDestination() {
    NavDestination() {
      List({ space: OrbitTokens.SPACE_MD }) {
        ListItem() {
          Column({ space: OrbitTokens.SPACE_SM }) {
            Row({ space: OrbitTokens.SPACE_SM }) {
              Text('Status')
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
              Blank()
              Text(this.cloudSyncSummaryLabel())
                .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
            }
            .width('100%')

            if (this.store.cloudSyncStatus !== null && this.store.cloudSyncStatus.state === 'ready') {
              Row({ space: OrbitTokens.SPACE_SM }) {
                Text('Synced Fleet')
                  .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                Blank()
                Text(
                  `${this.store.cloudSyncStatus.synced_host_count} hosts · ${this.store.cloudSyncStatus.synced_project_count} projects`
                )
                  .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
              }
              .width('100%')

              Row() {
                Text('Auto-Sync')
                  .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                  .layoutWeight(1)
                Toggle({
                  type: ToggleType.Switch,
                  isOn: this.store.cloudSyncStatus.auto_sync_enabled
                })
                  .onChange((isOn: boolean) => {
                    this.store.cloudSyncSetAutoSync(isOn);
                  })
              }
              .width('100%')
              .alignItems(VerticalAlign.Center)
            }

            if (
              this.store.cloudSyncStatus !== null &&
              this.store.cloudSyncStatus.last_error !== null &&
              this.store.cloudSyncStatus.last_error.length > 0
            ) {
              Text(this.store.cloudSyncStatus.last_error)
                .attributeModifier(orbitTextMeta(this.store.palette.statusDanger))
                .width('100%')
            }
          }
          .width('100%')
          .padding(OrbitTokens.SPACE_LG)
          .backgroundColor(this.store.palette.listCard)
          .borderRadius(OrbitTokens.RADIUS_LG)
          .alignItems(HorizontalAlign.Start)
        }

        if (this.store.cloudSyncStatus === null || this.store.cloudSyncStatus.state === 'signed_out') {
          ListItem() {
            Column({ space: OrbitTokens.SPACE_SM }) {
              Text('GitHub Personal Access Token')
                .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium))
              Text('Requires gist scope. Vault payloads are compressed and AES-256-GCM encrypted locally before upload.')
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
              TextInput({ placeholder: 'ghp_…', text: this.cloudTokenDraft })
                .type(InputType.Password)
                .attributeModifier(
                  orbitInputMono(
                    this.store.palette.fgPrimary,
                    this.store.palette.fgMuted,
                    this.store.palette.subtleSurface
                  )
                )
                .height(42)
                .borderRadius(OrbitTokens.RADIUS_MD)
                .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
                .width('100%')
                .onChange((val: string) => {
                  this.cloudTokenDraft = val;
                })

              Button(this.store.cloudSyncBusy ? 'Connecting…' : 'Connect GitHub', {
                type: ButtonType.Normal,
                stateEffect: true
              })
                .attributeModifier(
                  orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
                )
                .width('100%')
                .height(OrbitTokens.HIT_MIN)
                .borderRadius(OrbitTokens.RADIUS_MD)
                .enabled(!this.store.cloudSyncBusy && this.cloudTokenDraft.trim().length > 0)
                .onClick(() => {
                  dismissHarmonyKeyboard(this.getUIContext());
                  const tokenVal = this.cloudTokenDraft;
                  this.store.cloudSyncLoginWithToken(tokenVal).then((ok: boolean) => {
                    if (ok) {
                      this.cloudTokenDraft = '';
                    }
                  });
                })
            }
            .width('100%')
            .padding(OrbitTokens.SPACE_LG)
            .backgroundColor(this.store.palette.listCard)
            .borderRadius(OrbitTokens.RADIUS_LG)
            .alignItems(HorizontalAlign.Start)
          }
        } else if (this.store.cloudSyncStatus.state === 'locked') {
          ListItem() {
            Column({ space: OrbitTokens.SPACE_SM }) {
              Row() {
                Text('Vault Passphrase')
                  .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium))
                  .layoutWeight(1)
                Button('Generate', { type: ButtonType.Capsule, stateEffect: true })
                  .attributeModifier(
                    orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.subtleSurface)
                  )
                  .height(30)
                  .padding({ left: 10, right: 10 })
                  .onClick(() => {
                    const generated = this.store.generateCloudSyncPassphrase();
                    if (generated.length > 0) {
                      this.cloudPassphraseDraft = generated;
                      this.store.copyToClipboard(generated, 'Generated & copied passphrase');
                    }
                  })
              }
              .width('100%')
              .alignItems(VerticalAlign.Center)

              TextInput({ placeholder: 'Enter E2EE passphrase', text: this.cloudPassphraseDraft })
                .attributeModifier(
                  orbitInputMono(
                    this.store.palette.fgPrimary,
                    this.store.palette.fgMuted,
                    this.store.palette.subtleSurface
                  )
                )
                .height(42)
                .borderRadius(OrbitTokens.RADIUS_MD)
                .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
                .width('100%')
                .onChange((val: string) => {
                  this.cloudPassphraseDraft = val;
                })

              Button(this.store.cloudSyncBusy ? 'Unlocking…' : 'Unlock & Sync', {
                type: ButtonType.Normal,
                stateEffect: true
              })
                .attributeModifier(
                  orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
                )
                .width('100%')
                .height(OrbitTokens.HIT_MIN)
                .borderRadius(OrbitTokens.RADIUS_MD)
                .enabled(!this.store.cloudSyncBusy && this.cloudPassphraseDraft.trim().length > 0)
                .onClick(() => {
                  dismissHarmonyKeyboard(this.getUIContext());
                  const passVal = this.cloudPassphraseDraft;
                  this.store.cloudSyncUnlockWithPassphrase(passVal).then((ok: boolean) => {
                    if (ok) {
                      this.cloudPassphraseDraft = '';
                    }
                  });
                })
            }
            .width('100%')
            .padding(OrbitTokens.SPACE_LG)
            .backgroundColor(this.store.palette.listCard)
            .borderRadius(OrbitTokens.RADIUS_LG)
            .alignItems(HorizontalAlign.Start)
          }
        } else {
          ListItem() {
            Button(this.store.cloudSyncBusy ? 'Syncing…' : 'Sync Now', {
              type: ButtonType.Normal,
              stateEffect: true
            })
              .attributeModifier(
                orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
              )
              .width('100%')
              .height(OrbitTokens.HIT_MIN)
              .borderRadius(OrbitTokens.RADIUS_LG)
              .enabled(!this.store.cloudSyncBusy)
              .onClick(() => {
                this.store.cloudSyncNow();
              })
          }
        }

        if (this.store.cloudSyncStatus !== null && this.store.cloudSyncStatus.state !== 'signed_out') {
          ListItem() {
            Button('Disconnect Cloud Sync', { type: ButtonType.Normal, stateEffect: true })
              .attributeModifier(orbitButtonSubhead(this.store.palette.statusDanger, this.store.palette.listCard))
              .width('100%')
              .height(OrbitTokens.HIT_MIN)
              .borderRadius(OrbitTokens.RADIUS_LG)
              .onClick(() => {
                this.confirmDisconnectCloudSync();
              })
          }
        }
      }
      .width('100%')
      .height('100%')
      .padding({
        left: OrbitTokens.SPACE_LG,
        right: OrbitTokens.SPACE_LG,
        top: OrbitTokens.SPACE_MD,
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
    .title('Cloud Sync')
    .backgroundColor(this.store.palette.drawer)
  }

  @Builder
  private buildNavDestination(name: string, _param: Object | null) {
    if (name === 'host_detail' && this.inspectedHost !== null) {
      this.renderHostDetailDestination(this.inspectedHost)
    } else if (name === 'pair_host') {
      this.renderPairHostDestination()
    } else if (name === 'cloud_sync') {
      this.renderCloudSyncDestination()
    }
  }

  build() {
    Navigation(this.navPathStack) {
      this.renderRootSettings()
    }
    .title('Settings')
    .titleMode(NavigationTitleMode.Mini)
    .hideBackButton(true)
    .mode(NavigationMode.Stack)
    .navDestination(this.buildNavDestination)
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.drawer)
  }
}
