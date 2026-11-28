import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { orbitRuntime } from '../../model/OrbitRuntime';
import { HostCardDto } from '../../model/OrbitTypes';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitInputSubhead,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import { OrbitSectionHeader, dismissHarmonyKeyboard } from '../common/OrbitCommonControls';
import {
  OrbitSettingsDestinationId,
  OrbitSettingsNavParam,
  OrbitSettingsSearchHit,
  localizeSettingsSearchGroup,
  localizeSettingsSearchTitle,
  resolveOrbitSettingsInitialDestination,
  searchOrbitSettings
} from './OrbitSettingsNavModel';
import {
  OrbitSettingsAppearancePage,
  OrbitSettingsGeneralPage,
  OrbitSettingsNotificationsPage
} from './OrbitSettingsPreferencesPages';
import {
  OrbitSettingsCloudSyncPage,
  OrbitSettingsHostDetailPage,
  OrbitSettingsHostsPage,
  OrbitSettingsPairHostPage
} from './OrbitSettingsFleetPages';
import {
  OrbitSettingsAgentsModelsPage,
  OrbitSettingsPermissionsGuardianPage,
  OrbitSettingsPersonalAgentPage,
  OrbitSettingsUsageQuotasPage
} from './OrbitSettingsAgentsSafetyPages';
import {
  OrbitSettingsMcpPage,
  OrbitSettingsMemoryPage,
  OrbitSettingsPluginsPage,
  OrbitSettingsSandboxPage,
  OrbitSettingsSkillsPage
} from './OrbitSettingsExtensionsPages';

@ComponentV2
export struct OrbitSettingsRoot {
  @Param initialDestination: string = '';
  @Param initialHostId: string = '';

  @Local private store: OrbitHarmonyStore = orbitRuntime();
  private readonly navPathStack: NavPathStack = new NavPathStack();
  @Local private searchQuery: string = '';
  @Local private targetHostId: string = '';
  @Local private inspectedHostId: string = '';
  @Local private rootScopeEpoch: number = 0;
  private pageMounted: boolean = false;

  aboutToAppear(): void {
    this.pageMounted = true;
    this.targetHostId = this.initialHostId.trim().length > 0
      ? this.initialHostId.trim()
      : this.store.selectedHostId.trim();
    if (this.targetHostId.length === 0 && this.store.hosts.length > 0) {
      this.targetHostId = this.store.hosts[0].host_id;
    }
    this.store.refreshCloudSyncStatus();
    this.store.probeNotificationPermission();
    const initial = resolveOrbitSettingsInitialDestination(this.initialDestination);
    if (initial !== '') {
      this.pushDestination(initial, initial);
    }
  }

  aboutToDisappear(): void {
    this.pageMounted = false;
  }

  @Monitor('store.hostSelectionEpoch')
  onGlobalHostSelectionChanged(_mon: IMonitor): void {
    this.rootScopeEpoch += 1;
    const active = this.store.selectedHostId.trim();
    if (active.length > 0) {
      this.targetHostId = active;
    }
  }

  @Computed
  get resolvedHostId(): string {
    if (this.targetHostId.length > 0) {
      for (let i = 0; i < this.store.hosts.length; i++) {
        if (this.store.hosts[i].host_id === this.targetHostId) {
          return this.targetHostId;
        }
      }
    }
    if (this.store.selectedHostId.length > 0) {
      return this.store.selectedHostId;
    }
    return this.store.hosts.length > 0 ? this.store.hosts[0].host_id : '';
  }

  @Computed
  get searchHits(): OrbitSettingsSearchHit[] {
    return searchOrbitSettings(this.searchQuery, this.store.hosts.length > 0, true);
  }

  private pushDestination(dest: OrbitSettingsDestinationId, focusKey: string = ''): void {
    const param: OrbitSettingsNavParam = {
      hostId: this.resolvedHostId,
      focusKey: focusKey
    };
    this.navPathStack.pushPath({ name: dest, param: param });
    this.store.triggerHaptic('selection');
  }

  private pushHostDetail(hostId: string): void {
    this.inspectedHostId = hostId;
    const param: OrbitSettingsNavParam = {
      hostId: hostId,
      focusKey: 'host_detail'
    };
    this.navPathStack.pushPath({ name: 'host_detail', param: param });
  }

  private appearanceSummaryResource(): Resource {
    if (this.store.themeMode === 'light') {
      return $r('app.string.settings_appearance_light');
    }
    if (this.store.themeMode === 'dark') {
      return $r('app.string.settings_appearance_dark');
    }
    return $r('app.string.settings_appearance_system');
  }

  private languageSummaryResource(): Resource {
    if (this.store.appLanguage === 'en') {
      return $r('app.string.settings_lang_en');
    }
    if (this.store.appLanguage === 'zh_CN') {
      return $r('app.string.settings_lang_zh');
    }
    if (this.store.appLanguage === 'ja_JP') {
      return $r('app.string.settings_lang_ja');
    }
    return $r('app.string.settings_lang_system');
  }

  private cloudSyncSummaryResource(): Resource {
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

  @Builder
  private renderNavRow(
    icon: Resource,
    title: Resource,
    subtitle: Resource | string,
    dest: OrbitSettingsDestinationId
  ) {
    ListItem() {
      Button({ type: ButtonType.Normal, stateEffect: true }) {
        Row({ space: 10 }) {
          Row() {
            SymbolGlyph(icon)
              .fontSize(16)
              .fontColor([this.store.palette.iconPrimary])
          }
          .width(22)
          .justifyContent(FlexAlign.Center)

          Column({ space: 2 }) {
            Text(title)
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
              .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
            if (typeof subtitle === 'string') {
              if (subtitle.length > 0) {
                Text(subtitle)
                  .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                  .maxLines(1)
                  .textOverflow({ overflow: TextOverflow.Ellipsis })
              }
            } else {
              Text(subtitle)
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                .maxLines(1)
                .textOverflow({ overflow: TextOverflow.Ellipsis })
            }
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
      .constraintSize({ minHeight: 54 })
      .padding({
        left: OrbitTokens.SPACE_LG,
        right: OrbitTokens.SPACE_LG,
        top: OrbitTokens.SPACE_SM,
        bottom: OrbitTokens.SPACE_SM
      })
      .backgroundColor(Color.Transparent)
      .accessibilityText(title)
      .onClick(() => {
        if (!this.pageMounted) {
          return;
        }
        dismissHarmonyKeyboard(this.getUIContext());
        this.pushDestination(dest, dest);
      })
    }
  }

  @Builder
  private renderSearchBar() {
    Row({ space: OrbitTokens.SPACE_SM }) {
      SymbolGlyph($r('sys.symbol.magnifyingglass'))
        .fontSize(15)
        .fontColor([this.store.palette.fgMuted])

      TextInput({
        placeholder: $r('app.string.settings_search_placeholder'),
        text: this.searchQuery
      })
        .attributeModifier(
          orbitInputSubhead(
            this.store.palette.fgPrimary,
            this.store.palette.fgMuted,
            Color.Transparent
          )
        )
        .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
        .padding(0)
        .layoutWeight(1)
        .accessibilityText($r('app.string.settings_search_a11y'))
        .onChange((val: string) => {
          if (!this.pageMounted) {
            return;
          }
          this.searchQuery = val;
        })

      if (this.searchQuery.length > 0) {
        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.xmark'))
            .fontSize(12)
            .fontColor([this.store.palette.iconSecondary])
        }
        .width(28)
        .height(28)
        .padding(0)
        .backgroundColor(this.store.palette.subtleSurface)
        .accessibilityText($r('app.string.settings_search_clear_a11y'))
        .onClick(() => {
          if (!this.pageMounted) {
            return;
          }
          this.searchQuery = '';
          this.store.triggerHaptic('selection');
        })
      }
    }
    .width('100%')
    .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
    .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
    .backgroundColor(this.store.palette.listCard)
    .borderRadius(OrbitTokens.RADIUS_LG)
    .alignItems(VerticalAlign.Center)
  }

  @Builder
  private renderHostScopeSelector() {
    if (this.store.hosts.length > 1) {
      Column({ space: OrbitTokens.SPACE_XS }) {
        Text($r('app.string.settings_target_host_label'))
          .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
          .padding({ left: OrbitTokens.SPACE_XS })

        Scroll() {
          Row({ space: OrbitTokens.SPACE_XS }) {
            ForEach(
              this.store.hosts,
              (host: HostCardDto) => {
                Button(host.display_name, { type: ButtonType.Capsule, stateEffect: true })
                  .attributeModifier(
                    orbitButtonMeta(
                      this.resolvedHostId === host.host_id
                        ? this.store.palette.onAccent
                        : this.store.palette.fgSecondary,
                      this.resolvedHostId === host.host_id
                        ? this.store.palette.accentPrimaryFill
                        : this.store.palette.listCard
                    )
                  )
                  .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                  .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
                  .accessibilityText(host.display_name)
                  .onClick(() => {
                    if (!this.pageMounted) {
                      return;
                    }
                    this.targetHostId = host.host_id;
                    this.rootScopeEpoch += 1;
                    this.store.triggerHapticPreset('selection');
                  })
              },
              (host: HostCardDto) => `sthost_${host.host_id}`
            )
          }
        }
        .scrollable(ScrollDirection.Horizontal)
        .scrollBar(BarState.Off)
        .width('100%')
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
      .margin({ top: OrbitTokens.SPACE_SM })
    }
  }

  @Builder
  private renderRootSettings() {
    List({ space: 0 }) {
      // Search Bar
      ListItem() {
        this.renderSearchBar()
      }
      .margin({ bottom: OrbitTokens.SPACE_XS })

      if (this.searchQuery.trim().length > 0) {
        if (this.searchHits.length === 0) {
          ListItem() {
            Column({ space: OrbitTokens.SPACE_SM }) {
              Text($r('app.string.settings_search_empty'))
                .attributeModifier(orbitTextSubhead(this.store.palette.fgMuted))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              Button($r('app.string.settings_search_clear'), { type: ButtonType.Capsule, stateEffect: true })
                .attributeModifier(
                  orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.subtleSurface)
                )
                .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
                .accessibilityText($r('app.string.settings_search_clear_a11y'))
                .onClick(() => {
                  if (!this.pageMounted) {
                    return;
                  }
                  this.searchQuery = '';
                  this.store.triggerHaptic('selection');
                })
            }
            .width('100%')
            .padding(OrbitTokens.SPACE_XL)
            .backgroundColor(this.store.palette.listCard)
            .borderRadius(OrbitTokens.RADIUS_LG)
            .alignItems(HorizontalAlign.Center)
          }
          .margin({ top: OrbitTokens.SPACE_MD })
        } else {
          ListItemGroup() {
            ForEach(
              this.searchHits,
              (hit: OrbitSettingsSearchHit) => {
                ListItem() {
                  Button({ type: ButtonType.Normal, stateEffect: true }) {
                    Row({ space: OrbitTokens.SPACE_SM }) {
                      Column({ space: 2 }) {
                        Text(localizeSettingsSearchTitle(hit.entry, this.store.appLanguage))
                          .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                          .maxLines(2)
                        Text(localizeSettingsSearchGroup(hit.entry, this.store.appLanguage))
                          .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
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
                  .constraintSize({ minHeight: 54 })
                  .padding({
                    left: OrbitTokens.SPACE_LG,
                    right: OrbitTokens.SPACE_LG,
                    top: OrbitTokens.SPACE_SM,
                    bottom: OrbitTokens.SPACE_SM
                  })
                  .backgroundColor(Color.Transparent)
                  .accessibilityText(localizeSettingsSearchTitle(hit.entry, this.store.appLanguage))
                  .onClick(() => {
                    if (!this.pageMounted) {
                      return;
                    }
                    dismissHarmonyKeyboard(this.getUIContext());
                    if (hit.entry.destination === 'host_detail') {
                      if (this.resolvedHostId.length > 0) {
                        this.pushHostDetail(this.resolvedHostId);
                        this.store.triggerHapticPreset('selection');
                      } else {
                        this.pushDestination('hosts', 'hosts');
                      }
                    } else {
                      this.pushDestination(hit.entry.destination, hit.entry.focusKey);
                    }
                  })
                }
              },
              (hit: OrbitSettingsSearchHit) => `shit_${hit.entry.id}`
            )
          }
          .margin({ top: OrbitTokens.SPACE_SM })
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
      } else {
        ListItem() {
          this.renderHostScopeSelector()
        }

        // Group 1: Preferences
        ListItem() {
          OrbitSectionHeader({ title: $r('app.string.settings_group_preferences'), isFirst: true })
        }

        ListItemGroup() {
          this.renderNavRow(
            $r('sys.symbol.gearshape'),
            $r('app.string.settings_item_general'),
            this.languageSummaryResource(),
            'general'
          )
          this.renderNavRow(
            $r('sys.symbol.sun_max'),
            $r('app.string.settings_item_appearance'),
            this.appearanceSummaryResource(),
            'appearance'
          )
          this.renderNavRow(
            $r('sys.symbol.bell'),
            $r('app.string.settings_item_notifications'),
            this.store.pushPrefs.enabled
              ? $r('app.string.settings_state_enabled')
              : $r('app.string.settings_state_off'),
            'notifications'
          )
        }
        .backgroundColor(this.store.palette.listCard)
        .borderRadius(OrbitTokens.RADIUS_LG)
        .clip(true)
        .divider({
          strokeWidth: OrbitTokens.HAIRLINE,
          color: this.store.palette.hairline,
          startMargin: 48,
          endMargin: OrbitTokens.SPACE_LG
        })

        // Group 2: Fleet & Sync
        ListItem() {
          OrbitSectionHeader({ title: $r('app.string.settings_group_fleet_sync') })
        }

        ListItemGroup() {
          this.renderNavRow(
            $r('sys.symbol.display'),
            $r('app.string.settings_item_hosts'),
            $r('app.string.settings_hosts_count_summary', this.store.hosts.length),
            'hosts'
          )
          this.renderNavRow(
            $r('sys.symbol.cloud'),
            $r('app.string.settings_item_cloud_sync'),
            this.cloudSyncSummaryResource(),
            'cloud_sync'
          )
        }
        .backgroundColor(this.store.palette.listCard)
        .borderRadius(OrbitTokens.RADIUS_LG)
        .clip(true)
        .divider({
          strokeWidth: OrbitTokens.HAIRLINE,
          color: this.store.palette.hairline,
          startMargin: 48,
          endMargin: OrbitTokens.SPACE_LG
        })

        // Group 3: Agents & Safety
        ListItem() {
          OrbitSectionHeader({ title: $r('app.string.settings_group_agents_safety') })
        }

        ListItemGroup() {
          this.renderNavRow(
            $r('sys.symbol.link'),
            $r('app.string.nav_personal_router'),
            $r('app.string.settings_pa_row_subtitle', this.store.fleetMeshPeers.length),
            'personal_agent'
          )
          this.renderNavRow(
            $r('sys.symbol.wand_and_stars'),
            $r('app.string.settings_item_agents_models'),
            $r('app.string.settings_agents_models_subtitle'),
            'agents_models'
          )
          this.renderNavRow(
            $r('sys.symbol.checkmark_shield'),
            $r('app.string.settings_item_permissions_guardian'),
            $r('app.string.settings_permissions_subtitle'),
            'permissions_guardian'
          )
          this.renderNavRow(
            $r('sys.symbol.waveform'),
            $r('app.string.settings_item_usage_quotas'),
            $r('app.string.settings_usage_subtitle'),
            'usage_quotas'
          )
        }
        .backgroundColor(this.store.palette.listCard)
        .borderRadius(OrbitTokens.RADIUS_LG)
        .clip(true)
        .divider({
          strokeWidth: OrbitTokens.HAIRLINE,
          color: this.store.palette.hairline,
          startMargin: 48,
          endMargin: OrbitTokens.SPACE_LG
        })

        // Group 4: Context & Extensions
        ListItem() {
          OrbitSectionHeader({ title: $r('app.string.settings_group_context_extensions') })
        }

        ListItemGroup() {
          this.renderNavRow(
            $r('sys.symbol.wrench_and_screwdriver'),
            $r('app.string.settings_item_mcp'),
            $r('app.string.settings_mcp_subtitle'),
            'mcp'
          )
          this.renderNavRow(
            $r('sys.symbol.book'),
            $r('app.string.settings_item_skills'),
            $r('app.string.settings_skills_subtitle'),
            'skills'
          )
          this.renderNavRow(
            $r('sys.symbol.doc_plaintext'),
            $r('app.string.settings_item_memory'),
            $r('app.string.settings_memory_subtitle'),
            'memory'
          )
          this.renderNavRow(
            $r('sys.symbol.square_grid_2x2'),
            $r('app.string.settings_item_plugins'),
            $r('app.string.settings_plugins_subtitle'),
            'plugins'
          )
          this.renderNavRow(
            $r('sys.symbol.lock'),
            $r('app.string.settings_item_sandbox'),
            $r('app.string.settings_sandbox_subtitle'),
            'sandbox'
          )
        }
        .backgroundColor(this.store.palette.listCard)
        .borderRadius(OrbitTokens.RADIUS_LG)
        .clip(true)
        .divider({
          strokeWidth: OrbitTokens.HAIRLINE,
          color: this.store.palette.hairline,
          startMargin: 48,
          endMargin: OrbitTokens.SPACE_LG
        })

        // About
        ListItem() {
          OrbitSectionHeader({ title: $r('app.string.settings_group_about') })
        }

        ListItemGroup() {
          ListItem() {
            Row() {
              Text($r('app.string.sync_huks_security'))
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              Blank()
              Text(
                this.store.keyProtectionMode === 'huks_hardware'
                  ? $r('app.string.sync_huks_hardware')
                  : $r('app.string.sync_huks_fallback')
              )
                .attributeModifier(
                  orbitTextMono(
                    this.store.keyProtectionMode === 'huks_hardware'
                      ? this.store.palette.statusDirect
                      : this.store.palette.fgMuted
                  )
                )
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
            }
            .width('100%')
            .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
            .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
            .alignItems(VerticalAlign.Center)
          }

          ListItem() {
            Row() {
              Text($r('app.string.settings_about_encryption'))
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              Blank()
              Text($r('app.string.settings_about_encryption_value'))
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
              Text($r('app.string.settings_about_platform'))
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              Blank()
              Text($r('app.string.settings_about_platform_value'))
                .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
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
  private buildNavDestination(name: string, param: Object | null) {
    if (name === 'general') {
      OrbitSettingsGeneralPage()
    } else if (name === 'appearance') {
      OrbitSettingsAppearancePage()
    } else if (name === 'notifications') {
      OrbitSettingsNotificationsPage({
        hostId: this.resolvedHostId,
        rootEpoch: this.rootScopeEpoch
      })
    } else if (name === 'hosts') {
      OrbitSettingsHostsPage({
        onSelectHostDetail: (hostId: string) => {
          this.pushHostDetail(hostId);
        },
        onOpenPairHost: () => {
          this.pushDestination('pair_host', 'pair_host');
        }
      })
    } else if (name === 'host_detail') {
      OrbitSettingsHostDetailPage({
        hostId: (param !== null && (param as OrbitSettingsNavParam).hostId !== undefined)
          ? (param as OrbitSettingsNavParam).hostId
          : this.inspectedHostId,
        onPopBack: () => {
          this.navPathStack.pop();
        }
      })
    } else if (name === 'pair_host') {
      OrbitSettingsPairHostPage({
        onPopBack: () => {
          this.navPathStack.pop();
        }
      })
    } else if (name === 'cloud_sync') {
      OrbitSettingsCloudSyncPage()
    } else if (name === 'personal_agent') {
      OrbitSettingsPersonalAgentPage({
        hostId: this.resolvedHostId,
        rootEpoch: this.rootScopeEpoch
      })
    } else if (name === 'agents_models') {
      OrbitSettingsAgentsModelsPage({
        hostId: this.resolvedHostId,
        rootEpoch: this.rootScopeEpoch
      })
    } else if (name === 'permissions_guardian') {
      OrbitSettingsPermissionsGuardianPage({
        hostId: this.resolvedHostId,
        rootEpoch: this.rootScopeEpoch
      })
    } else if (name === 'usage_quotas') {
      OrbitSettingsUsageQuotasPage({
        hostId: this.resolvedHostId,
        rootEpoch: this.rootScopeEpoch
      })
    } else if (name === 'mcp') {
      OrbitSettingsMcpPage({
        hostId: this.resolvedHostId,
        rootEpoch: this.rootScopeEpoch
      })
    } else if (name === 'skills') {
      OrbitSettingsSkillsPage({
        hostId: this.resolvedHostId,
        rootEpoch: this.rootScopeEpoch
      })
    } else if (name === 'memory') {
      OrbitSettingsMemoryPage({
        hostId: this.resolvedHostId,
        rootEpoch: this.rootScopeEpoch
      })
    } else if (name === 'plugins') {
      OrbitSettingsPluginsPage({
        hostId: this.resolvedHostId,
        rootEpoch: this.rootScopeEpoch
      })
    } else if (name === 'sandbox') {
      OrbitSettingsSandboxPage({
        hostId: this.resolvedHostId,
        rootEpoch: this.rootScopeEpoch
      })
    }
  }

  build() {
    Navigation(this.navPathStack) {
      this.renderRootSettings()
    }
    .title($r('app.string.nav_settings'))
    .titleMode(NavigationTitleMode.Mini)
    .hideBackButton(true)
    .mode(NavigationMode.Stack)
    .navDestination(this.buildNavDestination)
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.drawer)
  }
}
