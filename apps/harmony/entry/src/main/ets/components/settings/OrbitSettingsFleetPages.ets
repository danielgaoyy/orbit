import {
  CapsuleSegmentButtonV2,
  LengthMetrics,
  SegmentButtonV2Items
} from '@kit.ArkUI';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { orbitRuntime } from '../../model/OrbitRuntime';
import {
  HostCardDto,
  formatConnectionStateLabel,
  formatRouteModeLabel
} from '../../model/OrbitTypes';
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
import {
  ORBIT_ROUTE_MODES,
  OrbitSettingsControlBinding,
  createSettingsControlBinding,
  isSettingsControlBindingActive
} from './OrbitSettingsNavModel';

interface HostsRowBinding {
  host: HostCardDto;
  control: OrbitSettingsControlBinding;
}

@ComponentV2
export struct OrbitSettingsHostsPage {
  @Event onSelectHostDetail: (hostId: string) => void = (_hostId: string) => {};
  @Event onOpenPairHost: () => void = () => {};

  private readonly store: OrbitHarmonyStore = orbitRuntime();
  @Local private fleetRelayDraft: string = '';
  @Local private relayStatusCount: number = -1;
  @Local private relayErrorText: string = '';
  @Local private visibleEpoch: number = 1;
  private pageMounted: boolean = false;
  private pageVisible: boolean = false;

  aboutToAppear(): void {
    this.pageMounted = true;
    this.pageVisible = true;
    this.visibleEpoch += 1;
    for (let i = 0; i < this.store.hosts.length; i++) {
      const url = this.store.hosts[i].relay_url;
      if (url !== null && url.trim().length > 0) {
        this.fleetRelayDraft = url.trim();
        break;
      }
    }
  }

  aboutToDisappear(): void {
    this.pageMounted = false;
    this.pageVisible = false;
    this.visibleEpoch += 1;
  }

  private hostsControlBindings(scopeKey: string): OrbitSettingsControlBinding[] {
    return [
      createSettingsControlBinding(
        this.store.selectedHostId.trim(),
        this.store.hostSelectionEpoch,
        0,
        this.visibleEpoch,
        scopeKey
      )
    ];
  }

  private isHostsControlActive(binding: OrbitSettingsControlBinding): boolean {
    return isSettingsControlBindingActive(
      binding,
      this.pageMounted,
      this.pageVisible,
      this.visibleEpoch,
      this.store.selectedHostId.trim(),
      this.store.hostSelectionEpoch,
      0,
      binding.scopeKey
    );
  }

  @Computed
  get boundHosts(): HostsRowBinding[] {
    const control = createSettingsControlBinding(
      this.store.selectedHostId.trim(),
      this.store.hostSelectionEpoch,
      0,
      this.visibleEpoch,
      'hosts_row'
    );
    const out: HostsRowBinding[] = [];
    for (let i = 0; i < this.store.hosts.length; i++) {
      out.push({ host: this.store.hosts[i], control: control });
    }
    return out;
  }

  private hostSummarySubtitle(host: HostCardDto): string {
    const state = formatConnectionStateLabel(host.connection_state, host.via_ssh_tunnel);
    return host.last_ping_ms !== null ? `${state} · ${host.last_ping_ms} ms` : state;
  }

  build() {
    NavDestination() {
      List({ space: 0 }) {
        ListItem() {
          OrbitSectionHeader({ title: $r('app.string.settings_hosts_paired_section'), isFirst: true })
        }

        ListItemGroup() {
          if (this.store.hosts.length === 0) {
            ListItem() {
              Row() {
                Text($r('app.string.settings_hosts_empty'))
                  .attributeModifier(orbitTextSubhead(this.store.palette.fgMuted))
                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              }
              .width('100%')
              .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
              .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
              .alignItems(VerticalAlign.Center)
            }
          } else {
            ForEach(
              this.boundHosts,
              (entry: HostsRowBinding) => {
                ListItem() {
                  Button({ type: ButtonType.Normal, stateEffect: true }) {
                    Row({ space: 10 }) {
                      Row() {
                        Row()
                          .width(8)
                          .height(8)
                          .borderRadius(4)
                          .backgroundColor(resolveHostStatusColor(entry.host, this.store))
                      }
                      .width(20)
                      .justifyContent(FlexAlign.Center)

                      Column({ space: 2 }) {
                        Row({ space: 6 }) {
                          Text(entry.host.display_name)
                            .attributeModifier(
                              orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium)
                            )
                            .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                            .maxLines(1)
                            .textOverflow({ overflow: TextOverflow.Ellipsis })
                            .flexShrink(1)

                          if (entry.host.host_id === this.store.selectedHostId && this.store.hosts.length > 1) {
                            Text($r('app.string.settings_hosts_active_badge'))
                              .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                              .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                              .padding({ left: 6, right: 6, top: 1, bottom: 1 })
                              .borderRadius(OrbitTokens.RADIUS_SM)
                              .backgroundColor(this.store.palette.subtleSurface)
                          }
                        }
                        .alignItems(VerticalAlign.Center)

                        Text(this.hostSummarySubtitle(entry.host))
                          .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
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
                    top: OrbitTokens.SPACE_SM,
                    bottom: OrbitTokens.SPACE_SM
                  })
                  .backgroundColor(Color.Transparent)
                  .accessibilityText(entry.host.display_name)
                  .onClick(() => {
                    if (!this.isHostsControlActive(entry.control)) {
                      return;
                    }
                    this.store.triggerHaptic('selection');
                    this.onSelectHostDetail(entry.host.host_id);
                  })
                }
              },
              (entry: HostsRowBinding) => `settings_host_${entry.control.renderKey}_${entry.host.host_id}`
            )
          }

          ForEach(
            this.hostsControlBindings('hosts_pair'),
            (ctrl: OrbitSettingsControlBinding) => {
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

                    Text($r('app.string.settings_pair_new_host'))
                      .attributeModifier(
                        orbitTextSubhead(this.store.palette.accentPrimary, FontWeight.Medium)
                      )
                      .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
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
                .padding({
                  left: OrbitTokens.SPACE_LG,
                  right: OrbitTokens.SPACE_LG,
                  top: OrbitTokens.SPACE_SM,
                  bottom: OrbitTokens.SPACE_SM
                })
                .backgroundColor(Color.Transparent)
                .accessibilityText($r('app.string.settings_pair_new_host'))
                .onClick(() => {
                  if (!this.isHostsControlActive(ctrl)) {
                    return;
                  }
                  this.store.triggerHaptic('selection');
                  this.onOpenPairHost();
                })
              }
            },
            (ctrl: OrbitSettingsControlBinding) => `hosts_pair_${ctrl.renderKey}`
          )
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

        // One-Click Fleet Relay Sync
        if (this.store.hosts.length > 0) {
          ListItem() {
            OrbitSectionHeader({ title: $r('app.string.settings_fleet_relay_section') })
          }

          ListItem() {
            Column({ space: OrbitTokens.SPACE_SM }) {
              Text($r('app.string.settings_fleet_relay_hint'))
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                .width('100%')

              ForEach(
                this.hostsControlBindings('hosts_relay'),
                (ctrl: OrbitSettingsControlBinding) => {
                  Column({ space: OrbitTokens.SPACE_SM }) {
                    TextInput({
                      placeholder: $r('app.string.settings_relay_url_placeholder'),
                      text: this.fleetRelayDraft
                    })
                      .attributeModifier(
                        orbitInputMono(
                          this.store.palette.fgPrimary,
                          this.store.palette.fgMuted,
                          this.store.palette.subtleSurface
                        )
                      )
                      .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                      .borderRadius(OrbitTokens.RADIUS_MD)
                      .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
                      .width('100%')
                      .accessibilityText($r('app.string.settings_fleet_relay_input_a11y'))
                      .onChange((val: string) => {
                        if (!this.isHostsControlActive(ctrl)) {
                          return;
                        }
                        this.fleetRelayDraft = val;
                      })

                    Button($r('app.string.settings_apply_relay_fleet'), { type: ButtonType.Normal, stateEffect: true })
                      .attributeModifier(
                        orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
                      )
                      .width('100%')
                      .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                      .borderRadius(OrbitTokens.RADIUS_MD)
                      .accessibilityText($r('app.string.settings_apply_relay_fleet'))
                      .onClick(() => {
                        if (!this.isHostsControlActive(ctrl)) {
                          return;
                        }
                        dismissHarmonyKeyboard(this.getUIContext());
                        const res = this.store.applyRelayToFleetScoped(this.fleetRelayDraft);
                        if (!this.isHostsControlActive(ctrl)) {
                          return;
                        }
                        if (res.ok && res.data !== null) {
                          this.relayStatusCount = res.data;
                          this.relayErrorText = '';
                          this.store.triggerHaptic('success');
                        } else {
                          this.relayStatusCount = -1;
                          this.relayErrorText = res.errorDetail;
                          this.store.triggerHaptic('warning');
                        }
                      })
                  }
                  .width('100%')
                },
                (ctrl: OrbitSettingsControlBinding) => `hosts_relay_${ctrl.renderKey}`
              )

              if (this.relayStatusCount >= 0) {
                Text($r('app.string.settings_apply_relay_fleet_done', this.relayStatusCount))
                  .attributeModifier(orbitTextMeta(this.store.palette.statusDirect))
                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                  .width('100%')
              }
              if (this.relayErrorText.length > 0) {
                Text(this.relayErrorText)
                  .attributeModifier(orbitTextMeta(this.store.palette.statusDanger))
                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                  .width('100%')
              }
            }
            .width('100%')
            .padding(OrbitTokens.SPACE_LG)
            .backgroundColor(this.store.palette.listCard)
            .borderRadius(OrbitTokens.RADIUS_LG)
            .alignItems(HorizontalAlign.Start)
          }
        }
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
    .title($r('app.string.settings_item_hosts'))
    .backgroundColor(this.store.palette.drawer)
    .onShown(() => {
      const wasHidden = !this.pageVisible;
      this.pageVisible = true;
      if (wasHidden) {
        this.visibleEpoch += 1;
      }
    })
    .onHidden(() => {
      this.pageVisible = false;
      this.visibleEpoch += 1;
    })
  }
}

@ComponentV2
export struct OrbitSettingsHostDetailPage {
  @Param hostId: string = '';
  @Event onPopBack: () => void = () => {};

  private readonly store: OrbitHarmonyStore = orbitRuntime();
  @Local private customNameDraft: string = '';
  @Local private routeModeDraft: string = 'auto';
  @Local private directEndpointDraft: string = '';
  @Local private relayUrlDraft: string = '';
  @Local private exportedPairingUri: string = '';
  @Local private exportedForHostId: string = '';
  @Local private exportErrorText: string = '';
  @Local private visibleEpoch: number = 1;
  @Local private hostBindEpoch: number = 1;
  private pageMounted: boolean = false;
  private pageVisible: boolean = false;
  @Local private routeModeItems: SegmentButtonV2Items = new SegmentButtonV2Items([
    { text: $r('app.string.settings_route_auto') },
    { text: $r('app.string.settings_route_direct') },
    { text: $r('app.string.settings_route_relay') }
  ]);

  @Computed
  get host(): HostCardDto | null {
    for (let i = 0; i < this.store.hosts.length; i++) {
      if (this.store.hosts[i].host_id === this.hostId) {
        return this.store.hosts[i];
      }
    }
    return null;
  }

  @Computed
  get routeModeIndex(): number {
    return ORBIT_ROUTE_MODES.indexOf(this.routeModeDraft);
  }

  aboutToAppear(): void {
    this.pageMounted = true;
    this.pageVisible = true;
    this.visibleEpoch += 1;
    this.syncDraftsForHost();
  }

  aboutToDisappear(): void {
    this.pageMounted = false;
    this.pageVisible = false;
    this.visibleEpoch += 1;
  }

  @Monitor('hostId')
  onHostIdChanged(_mon: IMonitor): void {
    this.hostBindEpoch += 1;
    // Clear any exported pairing QR from a previous host immediately
    this.exportedPairingUri = '';
    this.exportedForHostId = '';
    this.exportErrorText = '';
    this.syncDraftsForHost();
  }

  private syncDraftsForHost(): void {
    const h = this.host;
    if (h === null) {
      return;
    }
    this.customNameDraft = h.display_name;
    this.routeModeDraft = h.route_mode;
    this.directEndpointDraft = h.custom_direct_endpoint ?? '';
    this.relayUrlDraft = h.relay_url ?? '';
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

  private hostDetailControls(scopeKey: string): OrbitSettingsControlBinding[] {
    return [
      createSettingsControlBinding(
        this.hostId,
        this.store.hostSelectionEpoch,
        this.hostBindEpoch,
        this.visibleEpoch,
        scopeKey
      )
    ];
  }

  private isHostDetailControlActive(binding: OrbitSettingsControlBinding): boolean {
    return isSettingsControlBindingActive(
      binding,
      this.pageMounted,
      this.pageVisible,
      this.visibleEpoch,
      this.hostId,
      this.store.hostSelectionEpoch,
      this.hostBindEpoch,
      binding.scopeKey
    );
  }

  private selectRouteMode(binding: OrbitSettingsControlBinding, idx: number): void {
    if (!this.isHostDetailControlActive(binding)) {
      return;
    }
    if (idx >= 0 && idx < ORBIT_ROUTE_MODES.length) {
      this.routeModeDraft = ORBIT_ROUTE_MODES[idx];
      this.store.triggerHaptic('selection');
    }
  }

  private generatePairingQr(host: HostCardDto, binding: OrbitSettingsControlBinding): void {
    if (!this.isHostDetailControlActive(binding) || host.host_id !== binding.hostId) {
      return;
    }
    const targetId = binding.hostId;
    const res = this.store.exportHostPairingUriScoped(targetId);
    if (!this.isHostDetailControlActive(binding)) {
      return;
    }
    if (res.ok && res.data !== null) {
      this.exportedPairingUri = res.data;
      this.exportedForHostId = targetId;
      this.exportErrorText = '';
      this.store.triggerHaptic('success');
    } else {
      this.exportedPairingUri = '';
      this.exportedForHostId = '';
      this.exportErrorText = res.errorDetail;
      this.store.triggerHaptic('warning');
    }
  }

  private confirmRemoveHost(host: HostCardDto, binding: OrbitSettingsControlBinding): void {
    if (!this.isHostDetailControlActive(binding) || host.host_id !== binding.hostId) {
      return;
    }
    const capturedHostId = binding.hostId;
    this.getUIContext().showAlertDialog({
      title: $r('app.string.settings_remove_host_title', host.display_name),
      message: $r('app.string.settings_remove_host_message'),
      autoCancel: true,
      primaryButton: {
        value: $r('app.string.action_cancel'),
        fontColor: this.store.palette.fgSecondary,
        action: () => {}
      },
      secondaryButton: {
        value: $r('app.string.settings_remove_host_confirm'),
        fontColor: this.store.palette.statusDanger,
        action: () => {
          if (!this.isHostDetailControlActive(binding)) {
            return;
          }
          const removingLast = this.store.hosts.length <= 1;
          this.store.triggerHaptic('warning');
          this.store.removeHost(capturedHostId);
          this.onPopBack();
          if (removingLast) {
            this.store.showHostSettingsSheet = false;
          }
        }
      }
    });
  }

  build() {
    NavDestination() {
      if (this.host === null) {
        Column({ space: OrbitTokens.SPACE_SM }) {
          Text($r('app.string.settings_host_not_found'))
            .attributeModifier(orbitTextSubhead(this.store.palette.fgMuted))
            .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
        }
        .width('100%')
        .height('100%')
        .justifyContent(FlexAlign.Center)
        .alignItems(HorizontalAlign.Center)
      } else {
        List({ space: 0 }) {
          // 1. Diagnostics
          ListItem() {
            OrbitSectionHeader({ title: $r('app.string.settings_host_diagnostics'), isFirst: true })
          }

          ListItemGroup() {
            ListItem() {
              Row({ space: OrbitTokens.SPACE_SM }) {
                Row()
                  .width(8)
                  .height(8)
                  .borderRadius(4)
                  .backgroundColor(resolveHostStatusColor(this.host, this.store))

                Text(formatConnectionStateLabel(this.host.connection_state, this.host.via_ssh_tunnel))
                  .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                  .layoutWeight(1)

                ForEach(
                  this.hostDetailControls('host_reconnect'),
                  (ctrl: OrbitSettingsControlBinding) => {
                    Button($r('app.string.action_reconnect'), { type: ButtonType.Capsule, stateEffect: true })
                      .attributeModifier(
                        orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.subtleSurface)
                      )
                      .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                      .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
                      .accessibilityText($r('app.string.action_reconnect'))
                      .onClick(() => {
                        if (!this.isHostDetailControlActive(ctrl) || ctrl.hostId.length === 0) {
                          return;
                        }
                        this.store.triggerHaptic('action');
                        this.store.reconnectHost(ctrl.hostId);
                      })
                  },
                  (ctrl: OrbitSettingsControlBinding) => `host_reconn_${ctrl.renderKey}`
                )
              }
              .width('100%')
              .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
              .padding({
                left: OrbitTokens.SPACE_LG,
                right: OrbitTokens.SPACE_MD,
                top: OrbitTokens.SPACE_SM,
                bottom: OrbitTokens.SPACE_SM
              })
              .alignItems(VerticalAlign.Center)
            }

            ListItem() {
              Row() {
                Text($r('app.string.settings_host_latency'))
                  .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                Blank()
                Text(this.host.last_ping_ms !== null ? `${this.host.last_ping_ms} ms` : '—')
                  .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              }
              .width('100%')
              .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
              .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
              .alignItems(VerticalAlign.Center)
            }

            ListItem() {
              Row() {
                Text($r('app.string.settings_host_daemon'))
                  .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                Blank()
                Text(this.daemonSummary(this.host))
                  .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                  .maxLines(1)
                  .textOverflow({ overflow: TextOverflow.Ellipsis })
              }
              .width('100%')
              .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
              .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
              .alignItems(VerticalAlign.Center)
            }

            ForEach(
              this.hostDetailControls('host_pubkey'),
              (ctrl: OrbitSettingsControlBinding) => {
                ListItem() {
                  Button({ type: ButtonType.Normal, stateEffect: true }) {
                    Row({ space: 6 }) {
                      Text($r('app.string.settings_host_pubkey'))
                        .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                        .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                      Blank()
                      Text(`${this.host !== null ? this.host.daemon_pubkey_b64.substring(0, 16) : ''}…`)
                        .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                        .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
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
                  .accessibilityText($r('app.string.settings_host_pubkey'))
                  .onClick(() => {
                    if (
                      !this.isHostDetailControlActive(ctrl) ||
                      this.host === null ||
                      this.host.host_id !== ctrl.hostId
                    ) {
                      return;
                    }
                    this.store.copyToClipboard(this.host.daemon_pubkey_b64);
                  })
                }
              },
              (ctrl: OrbitSettingsControlBinding) => `host_pubkey_${ctrl.renderKey}`
            )

            if (this.host.last_error !== null && this.host.last_error.length > 0) {
              ListItem() {
                Text(this.host.last_error)
                  .attributeModifier(orbitTextMono(this.store.palette.statusDanger))
                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
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

            ForEach(
              this.hostDetailControls('host_upgrade'),
              (ctrl: OrbitSettingsControlBinding) => {
                ListItem() {
                  Button($r('app.string.settings_upgrade_daemon'), { type: ButtonType.Normal, stateEffect: true })
                    .attributeModifier(orbitButtonSubhead(this.store.palette.accentPrimary, Color.Transparent))
                    .width('100%')
                    .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                    .accessibilityText($r('app.string.settings_upgrade_daemon'))
                    .onClick(() => {
                      if (!this.isHostDetailControlActive(ctrl) || ctrl.hostId.length === 0) {
                        return;
                      }
                      this.store.triggerHaptic('action');
                      this.store.upgradeHostDaemon(ctrl.hostId);
                    })
                }
              },
              (ctrl: OrbitSettingsControlBinding) => `host_upg_${ctrl.renderKey}`
            )

            if (this.host.host_id !== this.store.selectedHostId) {
              ForEach(
                this.hostDetailControls('host_switch'),
                (ctrl: OrbitSettingsControlBinding) => {
                  ListItem() {
                    Button($r('app.string.settings_switch_host'), { type: ButtonType.Normal, stateEffect: true })
                      .attributeModifier(orbitButtonSubhead(this.store.palette.accentPrimary, Color.Transparent))
                      .width('100%')
                      .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                      .accessibilityText($r('app.string.settings_switch_host'))
                      .onClick(() => {
                        if (
                          !this.isHostDetailControlActive(ctrl) ||
                          this.host === null ||
                          this.host.host_id !== ctrl.hostId
                        ) {
                          return;
                        }
                        this.store.selectHost(ctrl.hostId);
                        this.onPopBack();
                        this.store.triggerHaptic('selection');
                      })
                  }
                },
                (ctrl: OrbitSettingsControlBinding) => `host_sw_${ctrl.renderKey}`
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

          // 2. Share Pairing URI / QR Code
          ListItem() {
            OrbitSectionHeader({ title: $r('app.string.settings_share_pairing_section') })
          }

          ListItem() {
            Column({ space: OrbitTokens.SPACE_SM }) {
              Text($r('app.string.settings_share_pairing_desc'))
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                .width('100%')

              if (
                this.exportedPairingUri.length > 0 &&
                this.exportedForHostId === this.host.host_id
              ) {
                Column({ space: OrbitTokens.SPACE_SM }) {
                  Column() {
                    QRCode(this.exportedPairingUri)
                      .width(196)
                      .height(196)
                      .color('#14161D')
                      .backgroundColor('#FFFFFF')
                  }
                  .padding(OrbitTokens.SPACE_MD)
                  .backgroundColor('#FFFFFF')
                  .borderRadius(OrbitTokens.RADIUS_MD)

                  Text($r('app.string.settings_pairing_qr_warning'))
                    .attributeModifier(orbitTextMeta(this.store.palette.statusWarning))
                    .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                    .textAlign(TextAlign.Center)
                    .width('100%')

                  ForEach(
                    this.hostDetailControls('host_qr_actions'),
                    (ctrl: OrbitSettingsControlBinding) => {
                      Row({ space: OrbitTokens.SPACE_SM }) {
                        Button($r('app.string.settings_copy_pairing_uri'), {
                          type: ButtonType.Capsule,
                          stateEffect: true
                        })
                          .attributeModifier(
                            orbitButtonMeta(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
                          )
                          .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                          .layoutWeight(1)
                          .accessibilityText($r('app.string.settings_copy_pairing_uri'))
                          .onClick(() => {
                            if (
                              !this.isHostDetailControlActive(ctrl) ||
                              this.exportedForHostId !== ctrl.hostId ||
                              this.exportedPairingUri.length === 0
                            ) {
                              return;
                            }
                            this.store.copyToClipboard(this.exportedPairingUri);
                          })

                        Button($r('app.string.common_hide'), { type: ButtonType.Capsule, stateEffect: true })
                          .attributeModifier(
                            orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface)
                          )
                          .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                          .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
                          .accessibilityText($r('app.string.common_hide'))
                          .onClick(() => {
                            if (!this.isHostDetailControlActive(ctrl)) {
                              return;
                            }
                            this.exportedPairingUri = '';
                            this.exportedForHostId = '';
                            this.store.triggerHaptic('selection');
                          })
                      }
                      .width('100%')
                    },
                    (ctrl: OrbitSettingsControlBinding) => `host_qr_act_${ctrl.renderKey}`
                  )
                }
                .width('100%')
                .alignItems(HorizontalAlign.Center)
              } else {
                ForEach(
                  this.hostDetailControls('host_qr'),
                  (ctrl: OrbitSettingsControlBinding) => {
                    Button($r('app.string.settings_show_pairing_qr'), {
                      type: ButtonType.Normal,
                      stateEffect: true
                    })
                      .attributeModifier(
                        orbitButtonSubhead(this.store.palette.fgPrimary, this.store.palette.subtleSurface)
                      )
                      .width('100%')
                      .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                      .borderRadius(OrbitTokens.RADIUS_MD)
                      .accessibilityText($r('app.string.settings_show_pairing_qr'))
                      .onClick(() => {
                        if (this.host !== null) {
                          this.generatePairingQr(this.host, ctrl);
                        }
                      })
                  },
                  (ctrl: OrbitSettingsControlBinding) => `host_qr_${ctrl.renderKey}`
                )
              }

              if (this.exportErrorText.length > 0) {
                Text(this.exportErrorText)
                  .attributeModifier(orbitTextMeta(this.store.palette.statusDanger))
                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                  .width('100%')
              }
            }
            .width('100%')
            .padding(OrbitTokens.SPACE_LG)
            .backgroundColor(this.store.palette.listCard)
            .borderRadius(OrbitTokens.RADIUS_LG)
            .alignItems(HorizontalAlign.Start)
          }

          // 3. Connection
          ListItem() {
            OrbitSectionHeader({ title: $r('app.string.settings_host_connection') })
          }

          ListItemGroup() {
            ListItem() {
              Column({ space: OrbitTokens.SPACE_SM }) {
                Text($r('app.string.settings_host_route_mode'))
                  .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium))
                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)

                if (this.routeModeIndex >= 0) {
                  ForEach(
                    this.hostDetailControls('host_route_mode'),
                    (ctrl: OrbitSettingsControlBinding) => {
                      CapsuleSegmentButtonV2({
                        items: this.routeModeItems,
                        selectedIndex: this.routeModeIndex,
                        onItemClicked: (idx: number) => {
                          this.selectRouteMode(ctrl, idx);
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
                    },
                    (ctrl: OrbitSettingsControlBinding) => `host_route_${ctrl.renderKey}`
                  )
                } else {
                  Text(formatRouteModeLabel(this.routeModeDraft))
                    .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                    .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                }
              }
              .alignItems(HorizontalAlign.Start)
              .width('100%')
              .padding(OrbitTokens.SPACE_LG)
            }

            ListItem() {
              Column({ space: 4 }) {
                Text($r('app.string.settings_host_display_name'))
                  .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium))
                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                ForEach(
                  this.hostDetailControls('host_input_name'),
                  (ctrl: OrbitSettingsControlBinding) => {
                    TextInput({
                      placeholder: this.host !== null ? this.host.hostname : '',
                      text: this.customNameDraft
                    })
                      .attributeModifier(
                        orbitInputSubhead(
                          this.store.palette.fgPrimary,
                          this.store.palette.fgMuted,
                          Color.Transparent
                        )
                      )
                      .constraintSize({ minHeight: 36 })
                      .padding(0)
                      .width('100%')
                      .accessibilityText($r('app.string.settings_host_display_name_a11y'))
                      .onChange((v: string) => {
                        if (!this.isHostDetailControlActive(ctrl)) {
                          return;
                        }
                        this.customNameDraft = v;
                      })
                  },
                  (ctrl: OrbitSettingsControlBinding) => `host_in_name_${ctrl.renderKey}`
                )
              }
              .alignItems(HorizontalAlign.Start)
              .width('100%')
              .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG, top: 10, bottom: 10 })
            }

            ListItem() {
              Column({ space: 4 }) {
                Text($r('app.string.settings_host_direct_endpoint'))
                  .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium))
                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                ForEach(
                  this.hostDetailControls('host_input_direct'),
                  (ctrl: OrbitSettingsControlBinding) => {
                    TextInput({
                      placeholder: this.host !== null && this.host.direct_endpoints.length > 0
                        ? this.host.direct_endpoints[0]
                        : $r('app.string.settings_direct_endpoint_placeholder'),
                      text: this.directEndpointDraft
                    })
                      .attributeModifier(
                        orbitInputMono(
                          this.store.palette.fgPrimary,
                          this.store.palette.fgMuted,
                          Color.Transparent
                        )
                      )
                      .constraintSize({ minHeight: 36 })
                      .padding(0)
                      .width('100%')
                      .accessibilityText($r('app.string.settings_host_direct_endpoint_a11y'))
                      .onChange((v: string) => {
                        if (!this.isHostDetailControlActive(ctrl)) {
                          return;
                        }
                        this.directEndpointDraft = v;
                      })
                  },
                  (ctrl: OrbitSettingsControlBinding) => `host_in_dir_${ctrl.renderKey}`
                )
              }
              .alignItems(HorizontalAlign.Start)
              .width('100%')
              .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG, top: 10, bottom: 10 })
            }

            ListItem() {
              Column({ space: 4 }) {
                Text($r('app.string.settings_host_relay_url'))
                  .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium))
                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                ForEach(
                  this.hostDetailControls('host_input_relay'),
                  (ctrl: OrbitSettingsControlBinding) => {
                    TextInput({
                      placeholder: $r('app.string.settings_relay_url_placeholder'),
                      text: this.relayUrlDraft
                    })
                      .attributeModifier(
                        orbitInputMono(
                          this.store.palette.fgPrimary,
                          this.store.palette.fgMuted,
                          Color.Transparent
                        )
                      )
                      .constraintSize({ minHeight: 36 })
                      .padding(0)
                      .width('100%')
                      .accessibilityText($r('app.string.settings_host_relay_url_a11y'))
                      .onChange((v: string) => {
                        if (!this.isHostDetailControlActive(ctrl)) {
                          return;
                        }
                        this.relayUrlDraft = v;
                      })
                  },
                  (ctrl: OrbitSettingsControlBinding) => `host_in_rel_${ctrl.renderKey}`
                )
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

          ForEach(
            this.hostDetailControls('host_save'),
            (ctrl: OrbitSettingsControlBinding) => {
              ListItem() {
                Button($r('app.string.settings_save_reconnect'))
                  .attributeModifier(
                    orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
                  )
                  .width('100%')
                  .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                  .borderRadius(OrbitTokens.RADIUS_LG)
                  .accessibilityText($r('app.string.settings_save_reconnect'))
                  .onClick(() => {
                    if (!this.isHostDetailControlActive(ctrl) || this.host === null || this.host.host_id !== ctrl.hostId) {
                      return;
                    }
                    dismissHarmonyKeyboard(this.getUIContext());
                    this.store.triggerHaptic('success');
                    this.store.updateHostConfig(
                      ctrl.hostId,
                      this.customNameDraft,
                      this.routeModeDraft,
                      this.host.ssh_alias ?? '',
                      this.host.remote_daemon_port > 0 ? this.host.remote_daemon_port : 6868,
                      this.directEndpointDraft,
                      this.relayUrlDraft
                    );
                  })
              }
              .margin({ top: OrbitTokens.SPACE_LG })
            },
            (ctrl: OrbitSettingsControlBinding) => `host_save_${ctrl.renderKey}`
          )

          ForEach(
            this.hostDetailControls('host_remove'),
            (ctrl: OrbitSettingsControlBinding) => {
              ListItem() {
                Button($r('app.string.settings_remove_host'), { type: ButtonType.Normal, stateEffect: true })
                  .attributeModifier(orbitButtonSubhead(this.store.palette.statusDanger, this.store.palette.listCard))
                  .width('100%')
                  .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                  .borderRadius(OrbitTokens.RADIUS_LG)
                  .accessibilityText($r('app.string.settings_remove_host'))
                  .onClick(() => {
                    if (!this.isHostDetailControlActive(ctrl) || this.host === null || this.host.host_id !== ctrl.hostId) {
                      return;
                    }
                    dismissHarmonyKeyboard(this.getUIContext());
                    this.confirmRemoveHost(this.host, ctrl);
                  })
              }
              .margin({ top: OrbitTokens.SPACE_MD })
            },
            (ctrl: OrbitSettingsControlBinding) => `host_rem_${ctrl.renderKey}`
          )
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
    }
    .title(this.host !== null ? this.host.display_name : $r('app.string.settings_item_hosts'))
    .backgroundColor(this.store.palette.drawer)
    .onShown(() => {
      const wasHidden = !this.pageVisible;
      this.pageVisible = true;
      if (wasHidden) {
        this.visibleEpoch += 1;
      }
    })
    .onHidden(() => {
      this.pageVisible = false;
      this.visibleEpoch += 1;
    })
  }
}

@ComponentV2
export struct OrbitSettingsPairHostPage {
  @Event onPopBack: () => void = () => {};

  private readonly store: OrbitHarmonyStore = orbitRuntime();
  @Local private pairUriInput: string = '';
  @Local private visibleEpoch: number = 1;
  private pageMounted: boolean = false;
  private pageVisible: boolean = false;

  aboutToAppear(): void {
    this.pageMounted = true;
    this.pageVisible = true;
    this.visibleEpoch += 1;
  }

  aboutToDisappear(): void {
    this.pageMounted = false;
    this.pageVisible = false;
    this.visibleEpoch += 1;
  }

  private pairControlBindings(scopeKey: string): OrbitSettingsControlBinding[] {
    return [
      createSettingsControlBinding(
        this.store.selectedHostId.trim(),
        this.store.hostSelectionEpoch,
        0,
        this.visibleEpoch,
        scopeKey
      )
    ];
  }

  private isPairControlActive(binding: OrbitSettingsControlBinding): boolean {
    return isSettingsControlBindingActive(
      binding,
      this.pageMounted,
      this.pageVisible,
      this.visibleEpoch,
      this.store.selectedHostId.trim(),
      this.store.hostSelectionEpoch,
      0,
      binding.scopeKey
    );
  }

  build() {
    NavDestination() {
      List({ space: OrbitTokens.SPACE_LG }) {
        ForEach(
          this.pairControlBindings('pair_scan'),
          (ctrl: OrbitSettingsControlBinding) => {
            ListItem() {
              Button($r('app.string.settings_scan_qr_code'))
                .attributeModifier(
                  orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
                )
                .width('100%')
                .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                .borderRadius(OrbitTokens.RADIUS_LG)
                .accessibilityText($r('app.string.settings_scan_qr_code'))
                .onClick(() => {
                  if (!this.isPairControlActive(ctrl)) {
                    return;
                  }
                  this.store.triggerHaptic('action');
                  this.store.scanQrAndPair(this.getUIContext().getHostContext());
                })
            }
          },
          (ctrl: OrbitSettingsControlBinding) => `pair_scan_${ctrl.renderKey}`
        )

        ListItem() {
          Column({ space: OrbitTokens.SPACE_SM }) {
            Text($r('app.string.settings_pairing_uri_label'))
              .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium))
              .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)

            ForEach(
              this.pairControlBindings('pair_uri'),
              (ctrl: OrbitSettingsControlBinding) => {
                Row({ space: OrbitTokens.SPACE_SM }) {
                  TextInput({
                    placeholder: $r('app.string.settings_pairing_uri_placeholder'),
                    text: this.pairUriInput
                  })
                    .attributeModifier(
                      orbitInputMono(
                        this.store.palette.fgPrimary,
                        this.store.palette.fgMuted,
                        Color.Transparent
                      )
                    )
                    .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                    .padding(0)
                    .layoutWeight(1)
                    .accessibilityText($r('app.string.settings_pairing_uri_input_a11y'))
                    .onChange((val: string) => {
                      if (!this.isPairControlActive(ctrl)) {
                        return;
                      }
                      this.pairUriInput = val;
                    })
                    .onSubmit(() => {
                      if (!this.isPairControlActive(ctrl)) {
                        return;
                      }
                      if (this.pairUriInput.trim().length > 0) {
                        this.store.pairHost(this.pairUriInput);
                        this.pairUriInput = '';
                        this.onPopBack();
                      }
                    })

                  PasteButton({
                    icon: PasteIconStyle.LINES,
                    text: PasteDescription.PASTE,
                    buttonType: ButtonType.Capsule
                  })
                    .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                    .backgroundColor(this.store.palette.subtleSurface)
                    .fontColor(this.store.palette.fgPrimary)
                    .iconColor(this.store.palette.iconPrimary)
                    .onClick((_event: ClickEvent, result: PasteButtonOnClickResult) => {
                      if (!this.isPairControlActive(ctrl)) {
                        return;
                      }
                      if (result === PasteButtonOnClickResult.SUCCESS) {
                        this.store.pasteAuthorizedPairingUri((filled: string) => {
                          if (!this.isPairControlActive(ctrl)) {
                            return;
                          }
                          this.pairUriInput = filled;
                        });
                      }
                    })
                }
                .width('100%')
                .alignItems(VerticalAlign.Center)
              },
              (ctrl: OrbitSettingsControlBinding) => `pair_uri_${ctrl.renderKey}`
            )
          }
          .alignItems(HorizontalAlign.Start)
          .width('100%')
          .padding(OrbitTokens.SPACE_LG)
          .backgroundColor(this.store.palette.listCard)
          .borderRadius(OrbitTokens.RADIUS_LG)
        }

        ForEach(
          this.pairControlBindings('pair_submit'),
          (ctrl: OrbitSettingsControlBinding) => {
            ListItem() {
              Button($r('app.string.settings_pair_host_submit'))
                .attributeModifier(
                  orbitButtonSubhead(this.store.palette.fgPrimary, this.store.palette.listCard)
                )
                .width('100%')
                .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                .borderRadius(OrbitTokens.RADIUS_LG)
                .enabled(this.pairUriInput.trim().length > 0)
                .opacity(this.pairUriInput.trim().length > 0 ? 1.0 : 0.45)
                .accessibilityText($r('app.string.settings_pair_host_submit'))
                .onClick(() => {
                  if (!this.isPairControlActive(ctrl)) {
                    return;
                  }
                  dismissHarmonyKeyboard(this.getUIContext());
                  this.store.triggerHaptic('action');
                  this.store.pairHost(this.pairUriInput);
                  this.pairUriInput = '';
                  this.onPopBack();
                })
            }
          },
          (ctrl: OrbitSettingsControlBinding) => `pair_submit_${ctrl.renderKey}`
        )
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
    .title($r('app.string.settings_pair_new_host'))
    .backgroundColor(this.store.palette.drawer)
    .onShown(() => {
      const wasHidden = !this.pageVisible;
      this.pageVisible = true;
      if (wasHidden) {
        this.visibleEpoch += 1;
      }
    })
    .onHidden(() => {
      this.pageVisible = false;
      this.visibleEpoch += 1;
    })
  }
}

@ComponentV2
export struct OrbitSettingsCloudSyncPage {
  private readonly store: OrbitHarmonyStore = orbitRuntime();
  @Local private cloudTokenDraft: string = '';
  @Local private cloudPassphraseDraft: string = '';
  @Local private deviceLinkUriDraft: string = '';
  @Local private showDeviceLinkQr: boolean = false;
  @Local private visibleEpoch: number = 1;
  private pageMounted: boolean = false;
  private pageVisible: boolean = false;

  aboutToAppear(): void {
    this.pageMounted = true;
    this.pageVisible = true;
    this.visibleEpoch += 1;
    this.store.refreshCloudSyncStatus();
  }

  aboutToDisappear(): void {
    this.pageMounted = false;
    this.pageVisible = false;
    this.visibleEpoch += 1;
  }

  private syncControlBindings(scopeKey: string): OrbitSettingsControlBinding[] {
    return [
      createSettingsControlBinding(
        this.store.selectedHostId.trim(),
        this.store.hostSelectionEpoch,
        0,
        this.visibleEpoch,
        scopeKey
      )
    ];
  }

  private isSyncControlActive(binding: OrbitSettingsControlBinding): boolean {
    return isSettingsControlBindingActive(
      binding,
      this.pageMounted,
      this.pageVisible,
      this.visibleEpoch,
      this.store.selectedHostId.trim(),
      this.store.hostSelectionEpoch,
      0,
      binding.scopeKey
    );
  }

  private cloudSyncStatusResource(): Resource {
    const st = this.store.cloudSyncStatus;
    if (st === null || st.state === 'signed_out') {
      return $r('app.string.settings_sync_not_connected');
    }
    if (st.state === 'locked') {
      return $r('app.string.settings_sync_locked');
    }
    if (st.state === 'syncing') {
      return $r('app.string.settings_sync_syncing');
    }
    if (st.state === 'error') {
      return $r('app.string.settings_sync_error');
    }
    return $r('app.string.settings_sync_connected');
  }

  private confirmDisconnectCloudSync(binding: OrbitSettingsControlBinding): void {
    if (!this.isSyncControlActive(binding)) {
      return;
    }
    this.getUIContext().showAlertDialog({
      title: $r('app.string.settings_sync_disconnect_title'),
      message: $r('app.string.settings_sync_disconnect_message'),
      autoCancel: true,
      primaryButton: {
        value: $r('app.string.action_cancel'),
        fontColor: this.store.palette.fgSecondary,
        action: () => {}
      },
      secondaryButton: {
        value: $r('app.string.settings_sync_disconnect_confirm'),
        fontColor: this.store.palette.statusDanger,
        action: () => {
          if (!this.isSyncControlActive(binding)) {
            return;
          }
          this.store.triggerHaptic('warning');
          this.store.cloudSyncDisconnect(false);
        }
      }
    });
  }

  build() {
    NavDestination() {
      List({ space: OrbitTokens.SPACE_MD }) {
        ListItem() {
          Column({ space: OrbitTokens.SPACE_SM }) {
            Row({ space: OrbitTokens.SPACE_SM }) {
              Text($r('app.string.settings_sync_status_label'))
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              Blank()
              Text(this.cloudSyncStatusResource())
                .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
            }
            .width('100%')

            if (this.store.cloudSyncStatus !== null && this.store.cloudSyncStatus.state === 'ready') {
              Row({ space: OrbitTokens.SPACE_SM }) {
                Text($r('app.string.settings_sync_fleet_label'))
                  .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                Blank()
                Text(
                  $r(
                    'app.string.settings_sync_fleet_counts',
                    this.store.cloudSyncStatus.synced_host_count,
                    this.store.cloudSyncStatus.synced_project_count
                  )
                )
                  .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              }
              .width('100%')

              ForEach(
                this.syncControlBindings('sync_ready_toggles'),
                (ctrl: OrbitSettingsControlBinding) => {
                  Column({ space: OrbitTokens.SPACE_SM }) {
                    Row({ space: OrbitTokens.SPACE_SM }) {
                      Text($r('app.string.settings_sync_auto_sync'))
                        .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                        .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                        .layoutWeight(1)
                      Toggle({
                        type: ToggleType.Switch,
                        isOn: this.store.cloudSyncStatus !== null && this.store.cloudSyncStatus.auto_sync_enabled
                      })
                        .selectedColor(this.store.palette.accentPrimaryFill)
                        .accessibilityText($r('app.string.settings_sync_auto_sync'))
                        .onChange((isOn: boolean) => {
                          if (!this.isSyncControlActive(ctrl)) {
                            return;
                          }
                          this.store.cloudSyncSetAutoSync(isOn);
                        })
                    }
                    .width('100%')
                    .alignItems(VerticalAlign.Center)

                    Row({ space: OrbitTokens.SPACE_SM }) {
                      Text($r('app.string.sync_zero_passphrase'))
                        .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                        .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                        .layoutWeight(1)
                      Toggle({
                        type: ToggleType.Switch,
                        isOn: this.store.cloudSyncStatus !== null && this.store.cloudSyncStatus.instant_unlock_enabled !== false
                      })
                        .selectedColor(this.store.palette.accentPrimaryFill)
                        .accessibilityText($r('app.string.sync_zero_passphrase'))
                        .onChange((isOn: boolean) => {
                          if (!this.isSyncControlActive(ctrl)) {
                            return;
                          }
                          this.store.setCloudSyncManualPassphraseMode(!isOn);
                        })
                    }
                    .width('100%')
                    .alignItems(VerticalAlign.Center)
                  }
                  .width('100%')
                },
                (ctrl: OrbitSettingsControlBinding) => `sync_rt_${ctrl.renderKey}`
              )
            }

            if (
              this.store.cloudSyncStatus !== null &&
              this.store.cloudSyncStatus.last_error !== null &&
              this.store.cloudSyncStatus.last_error.length > 0
            ) {
              Text(this.store.cloudSyncStatus.last_error)
                .attributeModifier(orbitTextMeta(this.store.palette.statusDanger))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
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
              Text($r('app.string.sync_device_link_qr'))
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              Text($r('app.string.settings_sync_device_link_desc'))
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)

              ForEach(
                this.syncControlBindings('sync_device_link'),
                (ctrl: OrbitSettingsControlBinding) => {
                  Column({ space: OrbitTokens.SPACE_SM }) {
                    Button($r('app.string.sync_scan_device_link'), { type: ButtonType.Normal, stateEffect: true })
                      .attributeModifier(
                        orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
                      )
                      .width('100%')
                      .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                      .borderRadius(OrbitTokens.RADIUS_MD)
                      .enabled(!this.store.cloudSyncBusy)
                      .accessibilityText($r('app.string.sync_scan_device_link'))
                      .onClick(() => {
                        if (!this.isSyncControlActive(ctrl)) {
                          return;
                        }
                        this.store.scanDeviceLinkQr(this.getUIContext().getHostContext());
                      })

                    Row({ space: OrbitTokens.SPACE_SM }) {
                      TextInput({
                        placeholder: $r('app.string.settings_sync_link_uri_placeholder'),
                        text: this.deviceLinkUriDraft
                      })
                        .attributeModifier(
                          orbitInputMono(
                            this.store.palette.fgPrimary,
                            this.store.palette.fgMuted,
                            this.store.palette.subtleSurface
                          )
                        )
                        .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                        .borderRadius(OrbitTokens.RADIUS_MD)
                        .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
                        .layoutWeight(1)
                        .accessibilityText($r('app.string.settings_sync_link_uri_input_a11y'))
                        .onChange((val: string) => {
                          if (!this.isSyncControlActive(ctrl)) {
                            return;
                          }
                          this.deviceLinkUriDraft = val;
                        })

                      Button($r('app.string.action_confirm'), { type: ButtonType.Capsule, stateEffect: true })
                        .attributeModifier(
                          orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface)
                        )
                        .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                        .padding({ left: 12, right: 12 })
                        .enabled(!this.store.cloudSyncBusy && this.deviceLinkUriDraft.trim().length > 0)
                        .accessibilityText($r('app.string.action_confirm'))
                        .onClick(() => {
                          if (!this.isSyncControlActive(ctrl)) {
                            return;
                          }
                          dismissHarmonyKeyboard(this.getUIContext());
                          const uriVal = this.deviceLinkUriDraft;
                          this.store.importCloudSyncDeviceLinkUri(uriVal).then((ok: boolean) => {
                            if (ok && this.isSyncControlActive(ctrl)) {
                              this.deviceLinkUriDraft = '';
                            }
                          });
                        })
                    }
                    .width('100%')
                  }
                  .width('100%')
                },
                (ctrl: OrbitSettingsControlBinding) => `sync_dl_${ctrl.renderKey}`
              )
            }
            .width('100%')
            .padding(OrbitTokens.SPACE_LG)
            .backgroundColor(this.store.palette.listCard)
            .borderRadius(OrbitTokens.RADIUS_LG)
            .alignItems(HorizontalAlign.Start)
          }

          ListItem() {
            Column({ space: OrbitTokens.SPACE_SM }) {
              Text($r('app.string.sync_github_device_flow'))
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)

              ForEach(
                this.syncControlBindings('sync_gh_flow'),
                (ctrl: OrbitSettingsControlBinding) => {
                  Column({ space: OrbitTokens.SPACE_SM }) {
                    if (this.store.githubDeviceFlow !== null) {
                      Text(this.store.githubDeviceFlow.user_code)
                        .attributeModifier(orbitTextMono(this.store.palette.accentPrimary, FontWeight.Bold))
                        .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                      Text(this.store.githubDeviceFlow.verification_uri)
                        .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                        .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                        .copyOption(CopyOptions.LocalDevice)
                      Row({ space: OrbitTokens.SPACE_SM }) {
                        Button($r('app.string.action_copy'), { type: ButtonType.Capsule, stateEffect: true })
                          .attributeModifier(orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface))
                          .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                          .layoutWeight(1)
                          .accessibilityText($r('app.string.action_copy'))
                          .onClick(() => {
                            if (!this.isSyncControlActive(ctrl)) {
                              return;
                            }
                            this.store.copyToClipboard(this.store.githubDeviceFlow?.user_code ?? '');
                          })
                        Button($r('app.string.settings_sync_verify_signin'), {
                          type: ButtonType.Capsule,
                          stateEffect: true
                        })
                          .attributeModifier(
                            orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
                          )
                          .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                          .layoutWeight(1)
                          .accessibilityText($r('app.string.settings_sync_verify_signin'))
                          .onClick(() => {
                            if (!this.isSyncControlActive(ctrl)) {
                              return;
                            }
                            this.store.pollGithubDeviceFlow();
                          })
                      }
                      .width('100%')
                    } else {
                      Button($r('app.string.sync_github_device_flow'), { type: ButtonType.Normal, stateEffect: true })
                        .attributeModifier(
                          orbitButtonSubhead(this.store.palette.fgPrimary, this.store.palette.subtleSurface)
                        )
                        .width('100%')
                        .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                        .borderRadius(OrbitTokens.RADIUS_MD)
                        .enabled(!this.store.cloudSyncBusy)
                        .accessibilityText($r('app.string.sync_github_device_flow'))
                        .onClick(() => {
                          if (!this.isSyncControlActive(ctrl)) {
                            return;
                          }
                          this.store.startGithubDeviceFlow();
                        })
                    }
                  }
                  .width('100%')
                  .alignItems(HorizontalAlign.Start)
                },
                (ctrl: OrbitSettingsControlBinding) => `sync_gh_${ctrl.renderKey}`
              )
            }
            .width('100%')
            .padding(OrbitTokens.SPACE_LG)
            .backgroundColor(this.store.palette.listCard)
            .borderRadius(OrbitTokens.RADIUS_LG)
            .alignItems(HorizontalAlign.Start)
          }

          ListItem() {
            Column({ space: OrbitTokens.SPACE_SM }) {
              Text($r('app.string.settings_sync_pat_title'))
                .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              Text($r('app.string.settings_sync_pat_desc'))
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)

              ForEach(
                this.syncControlBindings('sync_pat'),
                (ctrl: OrbitSettingsControlBinding) => {
                  Column({ space: OrbitTokens.SPACE_SM }) {
                    TextInput({
                      placeholder: $r('app.string.settings_sync_pat_placeholder'),
                      text: this.cloudTokenDraft
                    })
                      .type(InputType.Password)
                      .attributeModifier(
                        orbitInputMono(
                          this.store.palette.fgPrimary,
                          this.store.palette.fgMuted,
                          this.store.palette.subtleSurface
                        )
                      )
                      .constraintSize({ minHeight: 42 })
                      .borderRadius(OrbitTokens.RADIUS_MD)
                      .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
                      .width('100%')
                      .accessibilityText($r('app.string.settings_sync_pat_input_a11y'))
                      .onChange((val: string) => {
                        if (!this.isSyncControlActive(ctrl)) {
                          return;
                        }
                        this.cloudTokenDraft = val;
                      })

                    Button(
                      this.store.cloudSyncBusy
                        ? $r('app.string.common_connecting')
                        : $r('app.string.settings_sync_connect_github'),
                      { type: ButtonType.Normal, stateEffect: true }
                    )
                      .attributeModifier(
                        orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
                      )
                      .width('100%')
                      .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                      .borderRadius(OrbitTokens.RADIUS_MD)
                      .enabled(!this.store.cloudSyncBusy && this.cloudTokenDraft.trim().length > 0)
                      .accessibilityText($r('app.string.settings_sync_connect_github'))
                      .onClick(() => {
                        if (!this.isSyncControlActive(ctrl)) {
                          return;
                        }
                        dismissHarmonyKeyboard(this.getUIContext());
                        const tokenVal = this.cloudTokenDraft;
                        this.store.cloudSyncLoginWithToken(tokenVal).then((ok: boolean) => {
                          if (ok && this.isSyncControlActive(ctrl)) {
                            this.cloudTokenDraft = '';
                          }
                        });
                      })
                  }
                  .width('100%')
                },
                (ctrl: OrbitSettingsControlBinding) => `sync_pat_${ctrl.renderKey}`
              )
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
              ForEach(
                this.syncControlBindings('sync_unlock'),
                (ctrl: OrbitSettingsControlBinding) => {
                  Column({ space: OrbitTokens.SPACE_SM }) {
                    Row() {
                      Text($r('app.string.settings_sync_passphrase_title'))
                        .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium))
                        .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                        .layoutWeight(1)
                      Button($r('app.string.settings_sync_generate'), { type: ButtonType.Capsule, stateEffect: true })
                        .attributeModifier(
                          orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.subtleSurface)
                        )
                        .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                        .padding({ left: 10, right: 10 })
                        .accessibilityText($r('app.string.settings_sync_generate'))
                        .onClick(() => {
                          if (!this.isSyncControlActive(ctrl)) {
                            return;
                          }
                          const generated = this.store.generateCloudSyncPassphrase();
                          if (generated.length > 0 && this.isSyncControlActive(ctrl)) {
                            this.cloudPassphraseDraft = generated;
                            this.store.copyToClipboard(generated);
                          }
                        })
                    }
                    .width('100%')
                    .alignItems(VerticalAlign.Center)

                    TextInput({
                      placeholder: $r('app.string.settings_sync_passphrase_placeholder'),
                      text: this.cloudPassphraseDraft
                    })
                      .attributeModifier(
                        orbitInputMono(
                          this.store.palette.fgPrimary,
                          this.store.palette.fgMuted,
                          this.store.palette.subtleSurface
                        )
                      )
                      .constraintSize({ minHeight: 42 })
                      .borderRadius(OrbitTokens.RADIUS_MD)
                      .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
                      .width('100%')
                      .accessibilityText($r('app.string.settings_sync_passphrase_input_a11y'))
                      .onChange((val: string) => {
                        if (!this.isSyncControlActive(ctrl)) {
                          return;
                        }
                        this.cloudPassphraseDraft = val;
                      })

                    Button(
                      this.store.cloudSyncBusy
                        ? $r('app.string.settings_sync_unlocking')
                        : $r('app.string.settings_sync_unlock'),
                      { type: ButtonType.Normal, stateEffect: true }
                    )
                      .attributeModifier(
                        orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
                      )
                      .width('100%')
                      .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                      .borderRadius(OrbitTokens.RADIUS_MD)
                      .enabled(!this.store.cloudSyncBusy && this.cloudPassphraseDraft.trim().length > 0)
                      .accessibilityText($r('app.string.settings_sync_unlock'))
                      .onClick(() => {
                        if (!this.isSyncControlActive(ctrl)) {
                          return;
                        }
                        dismissHarmonyKeyboard(this.getUIContext());
                        const passVal = this.cloudPassphraseDraft;
                        this.store.cloudSyncUnlockWithPassphrase(passVal).then((ok: boolean) => {
                          if (ok && this.isSyncControlActive(ctrl)) {
                            this.cloudPassphraseDraft = '';
                          }
                        });
                      })
                  }
                  .width('100%')
                },
                (ctrl: OrbitSettingsControlBinding) => `sync_unlk_${ctrl.renderKey}`
              )
            }
            .width('100%')
            .padding(OrbitTokens.SPACE_LG)
            .backgroundColor(this.store.palette.listCard)
            .borderRadius(OrbitTokens.RADIUS_LG)
            .alignItems(HorizontalAlign.Start)
          }
        } else {
          ForEach(
            this.syncControlBindings('sync_now'),
            (ctrl: OrbitSettingsControlBinding) => {
              ListItem() {
                Button(
                  this.store.cloudSyncBusy
                    ? $r('app.string.settings_sync_syncing')
                    : $r('app.string.settings_sync_now'),
                  { type: ButtonType.Normal, stateEffect: true }
                )
                  .attributeModifier(
                    orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
                  )
                  .width('100%')
                  .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                  .borderRadius(OrbitTokens.RADIUS_LG)
                  .enabled(!this.store.cloudSyncBusy)
                  .accessibilityText($r('app.string.settings_sync_now'))
                  .onClick(() => {
                    if (!this.isSyncControlActive(ctrl)) {
                      return;
                    }
                    this.store.cloudSyncNow();
                  })
              }
            },
            (ctrl: OrbitSettingsControlBinding) => `sync_now_${ctrl.renderKey}`
          )

          ListItem() {
            Column({ space: OrbitTokens.SPACE_SM }) {
              Text($r('app.string.sync_device_link_qr'))
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              Text($r('app.string.settings_sync_export_qr_desc'))
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)

              ForEach(
                this.syncControlBindings('sync_qr'),
                (ctrl: OrbitSettingsControlBinding) => {
                  Column({ space: OrbitTokens.SPACE_SM }) {
                    if (this.showDeviceLinkQr && this.store.deviceLinkUri.length > 0) {
                      Column({ space: OrbitTokens.SPACE_SM }) {
                        Column() {
                          QRCode(this.store.deviceLinkUri)
                            .width(180)
                            .height(180)
                            .color('#14161D')
                            .backgroundColor('#FFFFFF')
                        }
                        .padding(OrbitTokens.SPACE_MD)
                        .backgroundColor('#FFFFFF')
                        .borderRadius(OrbitTokens.RADIUS_MD)

                        Row({ space: OrbitTokens.SPACE_SM }) {
                          Button($r('app.string.action_copy'), { type: ButtonType.Capsule, stateEffect: true })
                            .attributeModifier(orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface))
                            .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                            .layoutWeight(1)
                            .accessibilityText($r('app.string.action_copy'))
                            .onClick(() => {
                              if (!this.isSyncControlActive(ctrl)) {
                                return;
                              }
                              this.store.copyToClipboard(this.store.deviceLinkUri);
                            })
                          Button($r('app.string.action_close'), { type: ButtonType.Capsule, stateEffect: true })
                            .attributeModifier(orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface))
                            .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                            .layoutWeight(1)
                            .accessibilityText($r('app.string.action_close'))
                            .onClick(() => {
                              if (!this.isSyncControlActive(ctrl)) {
                                return;
                              }
                              this.showDeviceLinkQr = false;
                            })
                        }
                        .width('100%')
                      }
                      .width('100%')
                      .alignItems(HorizontalAlign.Center)
                    } else {
                      Row({ space: OrbitTokens.SPACE_SM }) {
                        Button($r('app.string.sync_export_qr'), { type: ButtonType.Capsule, stateEffect: true })
                          .attributeModifier(
                            orbitButtonMeta(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
                          )
                          .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                          .layoutWeight(1)
                          .accessibilityText($r('app.string.sync_export_qr'))
                          .onClick(() => {
                            if (!this.isSyncControlActive(ctrl)) {
                              return;
                            }
                            const uri = this.store.exportCloudSyncDeviceLinkUri();
                            if (uri.length > 0 && this.isSyncControlActive(ctrl)) {
                              this.showDeviceLinkQr = true;
                            }
                          })
                        Button($r('app.string.sync_scan_device_link'), { type: ButtonType.Capsule, stateEffect: true })
                          .attributeModifier(orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface))
                          .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                          .layoutWeight(1)
                          .accessibilityText($r('app.string.sync_scan_device_link'))
                          .onClick(() => {
                            if (!this.isSyncControlActive(ctrl)) {
                              return;
                            }
                            this.store.scanDeviceLinkQr(this.getUIContext().getHostContext());
                          })
                      }
                      .width('100%')
                    }
                  }
                  .width('100%')
                },
                (ctrl: OrbitSettingsControlBinding) => `sync_qr_${ctrl.renderKey}`
              )
            }
            .width('100%')
            .padding(OrbitTokens.SPACE_LG)
            .backgroundColor(this.store.palette.listCard)
            .borderRadius(OrbitTokens.RADIUS_LG)
            .alignItems(HorizontalAlign.Start)
          }
        }

        if (this.store.cloudSyncStatus !== null && this.store.cloudSyncStatus.state !== 'signed_out') {
          ForEach(
            this.syncControlBindings('sync_disconnect'),
            (ctrl: OrbitSettingsControlBinding) => {
              ListItem() {
                Button($r('app.string.settings_sync_disconnect'), { type: ButtonType.Normal, stateEffect: true })
                  .attributeModifier(orbitButtonSubhead(this.store.palette.statusDanger, this.store.palette.listCard))
                  .width('100%')
                  .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                  .borderRadius(OrbitTokens.RADIUS_LG)
                  .accessibilityText($r('app.string.settings_sync_disconnect'))
                  .onClick(() => {
                    this.confirmDisconnectCloudSync(ctrl);
                  })
              }
            },
            (ctrl: OrbitSettingsControlBinding) => `sync_disc_${ctrl.renderKey}`
          )
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
    .title($r('app.string.settings_item_cloud_sync'))
    .backgroundColor(this.store.palette.drawer)
    .onShown(() => {
      const wasHidden = !this.pageVisible;
      this.pageVisible = true;
      if (wasHidden) {
        this.visibleEpoch += 1;
      }
    })
    .onHidden(() => {
      this.pageVisible = false;
      this.visibleEpoch += 1;
    })
  }
}
